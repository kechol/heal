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
//! After persisting the snapshot, `run_commit` writes one compact line
//! to stdout: that the snapshot was recorded, the current
//! Critical/High count, and a `heal check` nudge when those counts
//! aren't zero. Per-finding listings and the recalibration banner
//! moved to `heal metrics` / `heal check` so the post-commit output
//! never exceeds two lines.

use std::io::{IsTerminal, Read, Write};
use std::path::Path;

use crate::core::calibration::Calibration;
use crate::core::config::load_from_project;
use crate::core::eventlog::{Event, EventLog};
use crate::core::finding::Finding;
use crate::core::severity::Severity;
use crate::core::snapshot::{
    ansi_wrap, MetricsSnapshot, ANSI_CYAN, ANSI_GREEN, ANSI_RED, ANSI_YELLOW,
};
use crate::core::HealPaths;
use crate::observers::run_all;
use anyhow::Result;

use crate::cli::HookEvent;
use crate::snapshot;

pub fn run(project: &Path, event: HookEvent) -> Result<()> {
    // The hook may be invoked from settings.json on any project Claude
    // Code happens to be in — including ones that never ran `heal init`.
    // Skip silently so we don't materialise `.heal/` on a project the
    // user never opted into.
    let paths = HealPaths::new(project);
    if !paths.root().exists() {
        return Ok(());
    }
    // Edit / Stop fire on every Claude turn. Bury any internal failure
    // so the hook never blocks the agent loop — log-write errors,
    // unparseable stdin, etc. are not worth propagating. `Commit` keeps
    // the original error path: it's invoked from a git hook (`heal hook
    // commit`) where surfacing failure during local debugging matters.
    match event {
        HookEvent::Commit => run_commit(project, &paths, &EventLog::new(paths.logs_dir()))?,
        HookEvent::Edit | HookEvent::Stop => {
            // Stop intentionally does NOT emit a nudge: `MetricsSnapshot`
            // only updates on commit, so any turn-level Stop nudge would
            // either repeat itself or stay silent.
            let _ = run_log_only(&paths, event);
        }
    }
    Ok(())
}

fn run_log_only(paths: &HealPaths, event: HookEvent) -> Result<()> {
    let logs = EventLog::new(paths.logs_dir());
    let payload = capture_stdin()?;
    logs.append(&Event::new(event.as_str(), payload))?;
    Ok(())
}

fn run_commit(project: &Path, paths: &HealPaths, logs: &EventLog) -> Result<()> {
    // ConfigMissing is a v0.1 affordance — we still want a row in
    // snapshots/ so `heal metrics` doesn't think nothing happened, but
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

    // Run observers ONCE and classify ONCE — the snapshot writer's
    // severity tally and the nudge's per-finding render both consume
    // the same Vec.
    let reports = run_all(project, &cfg, None);
    let (calibration, findings) = snapshot::classify_with_calibration(paths, &cfg, &reports);
    let snap = snapshot::pack_with_delta(project, paths, &cfg, &reports, &findings);
    EventLog::new(paths.snapshots_dir()).append(&Event::new(
        HookEvent::Commit.as_str(),
        serde_json::to_value(&snap).expect("MetricsSnapshot serialization is infallible"),
    ))?;
    logs.append(&Event::new(
        HookEvent::Commit.as_str(),
        commit_log_payload(project),
    ))?;
    crate::core::compaction::compact_all(
        paths,
        &crate::core::compaction::CompactionPolicy::default(),
        chrono::Utc::now(),
    )
    .ok();
    // Best-effort nudge — the user just committed and the snapshot is
    // already persisted, so don't fail the hook on rendering issues.
    write_nudge(calibration.as_ref(), &findings, &mut std::io::stdout()).ok();
    Ok(())
}

/// Emit a single-line post-commit summary: snapshot recorded, current
/// Critical/High counts, and a `heal check` pointer when there's
/// something to act on. Stays silent on uncalibrated projects so a
/// fresh `heal init` flow doesn't pollute the commit output.
fn write_nudge(
    calibration: Option<&Calibration>,
    findings: &[Finding],
    out: &mut impl Write,
) -> Result<()> {
    if calibration.is_none() {
        return Ok(());
    }
    let critical = findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::Critical))
        .count();
    let high = findings
        .iter()
        .filter(|f| matches!(f.severity, Severity::High))
        .count();
    let colorize = std::io::stdout().is_terminal();

    if critical == 0 && high == 0 {
        writeln!(
            out,
            "heal: recorded · {}",
            ansi_wrap(ANSI_GREEN, "clean", colorize),
        )?;
        return Ok(());
    }

    let mut counts: Vec<String> = Vec::with_capacity(2);
    if critical > 0 {
        counts.push(ansi_wrap(
            ANSI_RED,
            &format!("{critical} critical"),
            colorize,
        ));
    }
    if high > 0 {
        counts.push(ansi_wrap(ANSI_YELLOW, &format!("{high} high"), colorize));
    }
    writeln!(
        out,
        "heal: recorded · {} · {}",
        counts.join(", "),
        ansi_wrap(ANSI_CYAN, "heal check", colorize),
    )?;
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
        let (calibration, findings) =
            crate::snapshot::classify_with_calibration(&paths, &cfg, &reports);

        let mut buf: Vec<u8> = Vec::new();
        write_nudge(calibration.as_ref(), &findings, &mut buf).unwrap();
        assert!(
            buf.is_empty(),
            "no calibration → no nudge, got: {}",
            String::from_utf8_lossy(&buf),
        );
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn nudge_summarises_critical_count_in_one_line() {
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
        crate::commands::calibrate::run(dir.path(), false, false).unwrap();
        let reports = run_all(dir.path(), &cfg, None);
        let (calibration, findings) =
            crate::snapshot::classify_with_calibration(&paths, &cfg, &reports);

        let mut buf: Vec<u8> = Vec::new();
        write_nudge(calibration.as_ref(), &findings, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert_eq!(
            out.lines().count(),
            1,
            "post-commit summary must be a single line, got: {out}",
        );
        assert!(
            out.starts_with("heal: recorded · "),
            "unexpected prefix: {out}"
        );
        assert!(
            out.contains("critical") || out.contains("high"),
            "expected a critical/high count, got: {out}",
        );
        assert!(
            out.contains("heal check"),
            "missing nudge to heal check: {out}"
        );
    }

    #[test]
    fn nudge_says_clean_when_no_critical_or_high() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "lib.rs", "fn ok() {}\n", "a@b.c", "init");
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        let cfg = crate::core::config::Config::default();
        cfg.save(&paths.config()).unwrap();
        crate::commands::calibrate::run(dir.path(), false, false).unwrap();
        let reports = run_all(dir.path(), &cfg, None);
        let (calibration, findings) =
            crate::snapshot::classify_with_calibration(&paths, &cfg, &reports);

        let mut buf: Vec<u8> = Vec::new();
        write_nudge(calibration.as_ref(), &findings, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert_eq!(out.trim_end(), "heal: recorded · clean");
    }
}
