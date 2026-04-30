//! `heal hook <commit|edit|stop>` — single entrypoint invoked by git hooks
//! and the Claude plugin. Each event has a different write target:
//!
//! | event  | snapshots/ | logs/ | observer scan | stdin payload |
//! | ------ | :--------: | :---: | :-----------: | :-----------: |
//! | commit |     ✓      |   ✓   |       ✓       |       —       |
//! | edit   |     —      |   ✓   |       —       |       ✓       |
//! | stop   |     —      |   ✓   |       —       |       ✓       |
//!
//! `commit` is the only event that runs observers (heavy work) — `edit` and
//! `stop` stay below ~1ms so the Claude plugin loop isn't slowed down.
//! `commit` also writes a lightweight metadata record to `logs/` so the
//! event timeline (`heal logs`) is the single source of truth for "what
//! happened when", while `snapshots/` retains the typed metric series.
//!
//! ## Post-commit nudge
//!
//! After persisting the snapshot, `run_commit` writes a short nudge to
//! stdout listing every `Severity::Critical` and `Severity::High`
//! finding (Medium and Ok are silent). Hotspot-flagged entries lead.
//! No cool-down or dedup — the same problem reappears every commit
//! until it's fixed, which is the point. `RecalibrationCheck` is
//! consulted opportunistically; if it fires, a single hint line is
//! prepended.

use std::io::{IsTerminal, Read, Write};
use std::path::Path;

use crate::core::calibration::{Calibration, RecalibrationCheck};
use crate::core::config::{load_from_project, Config};
use crate::core::eventlog::{Event, EventLog};
use crate::core::finding::Finding;
use crate::core::severity::Severity;
use crate::core::snapshot::{ansi_wrap, MetricsSnapshot, ANSI_RED, ANSI_YELLOW};
use crate::core::HealPaths;
use crate::observers::{classify, run_all, ObserverReports};
use anyhow::Result;

use crate::cli::HookEvent;
use crate::snapshot;

pub fn run(project: &Path, event: HookEvent) -> Result<()> {
    let paths = HealPaths::new(project);
    let logs = EventLog::new(paths.logs_dir());

    match event {
        HookEvent::Commit => run_commit(project, &paths, &logs)?,
        HookEvent::Edit | HookEvent::Stop => {
            // Both events stay log-only. Stop intentionally does NOT emit a
            // nudge: `MetricsSnapshot` only updates on commit, so any
            // turn-level Stop nudge would either repeat itself or stay
            // silent.
            logs.append(&Event::new(event.as_str(), capture_stdin()?))?;
        }
    }
    Ok(())
}

fn run_commit(project: &Path, paths: &HealPaths, logs: &EventLog) -> Result<()> {
    // ConfigMissing is a v0.1 affordance — we still want a row in
    // snapshots/ so `heal status` doesn't think nothing happened, but
    // there's nothing to scan or nudge about until `heal init` lands.
    let cfg = match load_from_project(project) {
        Ok(c) => c,
        Err(crate::core::Error::ConfigMissing(_)) => {
            EventLog::new(paths.snapshots_dir()).append(&Event::new(
                HookEvent::Commit.as_str(),
                serde_json::to_value(MetricsSnapshot::default())
                    .expect("MetricsSnapshot serialization is infallible"),
            ))?;
            logs.append(&Event::new(
                HookEvent::Commit.as_str(),
                commit_log_payload(project),
            ))?;
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    // Run observers ONCE and feed the same `reports` to the snapshot
    // writer and the nudge — both consumers do the same heavy work
    // otherwise.
    let reports = run_all(project, &cfg, None);
    let snap = snapshot::pack_with_delta(project, paths, &cfg, &reports);
    EventLog::new(paths.snapshots_dir()).append(&Event::new(
        HookEvent::Commit.as_str(),
        serde_json::to_value(&snap).expect("MetricsSnapshot serialization is infallible"),
    ))?;
    logs.append(&Event::new(
        HookEvent::Commit.as_str(),
        commit_log_payload(project),
    ))?;
    // Best-effort nudge — the user just committed and the snapshot is
    // already persisted, so don't fail the hook on rendering issues.
    write_nudge(paths, &cfg, &reports, &mut std::io::stdout()).ok();
    Ok(())
}

/// Compose and emit the post-commit Severity nudge to `out`.
///
/// `reports` is the same `run_all` output that built the just-written
/// snapshot — the caller threads it through so we don't run observers
/// twice on a single commit.
fn write_nudge(
    paths: &HealPaths,
    cfg: &Config,
    reports: &ObserverReports,
    out: &mut impl Write,
) -> Result<()> {
    let Ok(calibration) = Calibration::load(&paths.calibration()) else {
        // No calibration yet — nothing actionable to nudge about.
        return Ok(());
    };
    let calibration = calibration.with_overrides(cfg);
    let findings = classify(reports, &calibration);

    // Recalibration banner first so the user sees it before the
    // per-finding lines, even when Critical/High is empty.
    let snapshots = EventLog::new(paths.snapshots_dir());
    let check = RecalibrationCheck::evaluate(&snapshots, &calibration, chrono::Utc::now());
    let colorize = std::io::stdout().is_terminal();
    if check.fired() {
        if let Some(days) = check.age_exceeded_days {
            writeln!(
                out,
                "{} recalibration suggested ({days} days since last calibration)",
                ansi_wrap(ANSI_YELLOW, "note:", colorize),
            )?;
        }
        if let Some(pct) = check.file_count_delta_pct {
            writeln!(
                out,
                "{} recalibration suggested (codebase size {:+.0}% since last calibration)",
                ansi_wrap(ANSI_YELLOW, "note:", colorize),
                pct * 100.0,
            )?;
        }
        if let Some(streak) = check.critical_clean_streak_days {
            writeln!(
                out,
                "{} recalibration suggested ({streak} days of zero Critical — thresholds may be too lenient)",
                ansi_wrap(ANSI_YELLOW, "note:", colorize),
            )?;
        }
    }

    let mut surfaced: Vec<&Finding> = findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::Critical | Severity::High))
        .collect();
    if surfaced.is_empty() {
        return Ok(());
    }
    // hotspot=true first; then Critical before High; then by file for
    // determinism. Matches the `heal check` ordering.
    surfaced.sort_by(|a, b| {
        b.hotspot
            .cmp(&a.hotspot)
            .then_with(|| b.severity.cmp(&a.severity))
            .then_with(|| a.location.file.cmp(&b.location.file))
    });

    writeln!(out)?;
    for f in &surfaced {
        let label = match (f.severity, f.hotspot) {
            (Severity::Critical, true) => ansi_wrap(ANSI_RED, "🔴 Critical 🔥", colorize),
            (Severity::Critical, false) => ansi_wrap(ANSI_RED, "🔴 Critical", colorize),
            (Severity::High, true) => ansi_wrap(ANSI_YELLOW, "🟠 High 🔥", colorize),
            (Severity::High, false) => ansi_wrap(ANSI_YELLOW, "🟠 High", colorize),
            _ => continue,
        };
        writeln!(
            out,
            "{label} {} {}",
            f.location.file.display(),
            f.short_label(),
        )?;
    }
    writeln!(out, "Next: `heal check` / `claude /heal-fix`")?;
    Ok(())
}

/// Lightweight snapshot of the just-recorded commit (sha, parent, author,
/// subject, file/line change counts). Pure metadata — the heavy metric
/// payload lives in `snapshots/`. A failed lookup is logged to stderr but
/// returns `Value::Null` so the post-commit hook never aborts the commit.
fn commit_log_payload(project: &Path) -> serde_json::Value {
    let Some(info) = crate::observer::git::head_commit_info(project) else {
        eprintln!("heal: commit metadata unavailable (HEAD missing or not a git repo)");
        return serde_json::Value::Null;
    };
    serde_json::to_value(&info).expect("CommitInfo serialization is infallible")
}

fn capture_stdin() -> Result<serde_json::Value> {
    // Claude plugin hooks deliver event metadata via stdin (JSON). Skip the
    // read on a tty so manual invocations don't block on user input.
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Ok(serde_json::Value::Null);
    }
    let mut buf = String::new();
    stdin.lock().read_to_string(&mut buf)?;
    if buf.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    Ok(match serde_json::from_str(&buf) {
        Ok(v) => v,
        Err(_) => serde_json::Value::String(buf),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{commit, init_repo};
    use tempfile::TempDir;

    fn read_log_events(paths: &HealPaths) -> Vec<Event> {
        EventLog::new(paths.logs_dir())
            .try_iter()
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    #[test]
    fn commit_writes_to_both_snapshots_and_logs() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(
            dir.path(),
            "lib.rs",
            "fn ok() {}\n",
            "alice@example.com",
            "feat: add ok",
        );
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        // Commit hook needs `.heal/config.toml` to drive observers; init flow
        // would normally create it, but we exercise the hook in isolation.
        std::fs::write(paths.config(), "").unwrap();

        run(dir.path(), HookEvent::Commit).unwrap();

        let snap_events: Vec<Event> = EventLog::new(paths.snapshots_dir())
            .try_iter()
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(snap_events.len(), 1);
        assert_eq!(snap_events[0].event, HookEvent::Commit.as_str());

        let log_events = read_log_events(&paths);
        assert_eq!(log_events.len(), 1);
        assert_eq!(log_events[0].event, HookEvent::Commit.as_str());
        let info: crate::observer::git::CommitInfo =
            serde_json::from_value(log_events[0].data.clone()).unwrap();
        assert_eq!(info.author_email.as_deref(), Some("alice@example.com"));
        assert_eq!(info.message_summary, "feat: add ok");
        assert_eq!(info.files_changed, 1);
        assert!(info.insertions >= 1);
    }

    #[test]
    fn edit_only_writes_to_logs() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        run(dir.path(), HookEvent::Edit).unwrap();

        let snap_files: usize = std::fs::read_dir(paths.snapshots_dir()).unwrap().count();
        assert_eq!(snap_files, 0);

        let log_events = read_log_events(&paths);
        assert_eq!(log_events.len(), 1);
        assert_eq!(log_events[0].event, HookEvent::Edit.as_str());
        assert!(log_events[0].data.is_null());
    }

    #[test]
    fn stop_only_writes_to_logs() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        run(dir.path(), HookEvent::Stop).unwrap();
        let log_events = read_log_events(&paths);
        assert_eq!(log_events.len(), 1);
        assert_eq!(log_events[0].event, HookEvent::Stop.as_str());
    }

    /// `write_nudge` must stay silent on a fresh project with no
    /// calibration on disk — the post-commit hook fires before the
    /// user has run `heal init`/`heal calibrate`, and we never want to
    /// pollute the commit output with a stack trace or empty banner.
    #[test]
    fn nudge_is_silent_without_calibration() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "lib.rs", "fn ok(){}\n", "a@b.c", "init");
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        let cfg = crate::core::config::Config::default();
        cfg.save(&paths.config()).unwrap();
        let reports = run_all(dir.path(), &cfg, None);

        let mut buf: Vec<u8> = Vec::new();
        write_nudge(&paths, &cfg, &reports, &mut buf).unwrap();
        assert!(
            buf.is_empty(),
            "no calibration → no nudge, got: {}",
            String::from_utf8_lossy(&buf),
        );
    }

    #[test]
    fn nudge_lists_critical_finding_with_metric_label() {
        // Synthesize a project where the only function trips the
        // calibration's floor, so write_nudge has something to surface.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        // A nest of `if`s drives CCN past the FLOOR_CCN=25 threshold.
        let mut src = String::from("fn busy(x: i32) -> i32 {\n");
        for _ in 0..30 {
            src.push_str("    if x > 0 { return x; }\n");
        }
        src.push_str("    0\n}\n");
        commit(dir.path(), "lib.rs", &src, "a@b.c", "init");

        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        let cfg = crate::core::config::Config::default();
        cfg.save(&paths.config()).unwrap();
        // Calibrate inline so a calibration.toml exists.
        crate::commands::calibrate::run(dir.path(), Some("test".into()), false).unwrap();
        let reports = run_all(dir.path(), &cfg, None);

        let mut buf: Vec<u8> = Vec::new();
        write_nudge(&paths, &cfg, &reports, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(
            out.contains("Critical") || out.contains("High"),
            "expected a Severity nudge line, got: {out}",
        );
        assert!(
            out.contains("CCN=") || out.contains("Cognitive="),
            "metric value missing from nudge: {out}",
        );
        assert!(out.contains("Next:"), "next-steps hint missing: {out}");
    }
}
