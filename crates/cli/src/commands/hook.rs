//! `heal hook <commit|edit|stop>` — single entrypoint invoked by git
//! hooks and Claude Code's `settings.json` hook commands.
//!
//! - `commit` runs every observer, classifies the result against the
//!   project's calibration, and emits a one-line nudge. No event-log
//!   write — `latest.json` (refreshed on every `heal status`) is the
//!   live state of record.
//! - `edit` / `stop` are no-ops kept for back-compat with any
//!   `settings.json` registrations that survived an upgrade.
//!   `heal skills install` actively sweeps such entries.
//!
//! ## Post-commit nudge
//!
//! `run_commit` writes one compact line to stdout: a "recorded" marker,
//! the current Critical/High count, and a `heal status` pointer when
//! those counts aren't zero. Stays silent on uncalibrated projects so
//! a fresh `heal init` flow doesn't pollute the commit output.

use std::io::{IsTerminal, Write};
use std::path::Path;

use crate::core::accepted::{decorate_findings, read_accepted};
use crate::core::calibration::Calibration;
use crate::core::config::{load_from_project, Config};
use crate::core::finding::Finding;
use crate::core::severity::Severity;
use crate::core::term::{ansi_wrap, ANSI_CYAN, ANSI_GREEN, ANSI_RED, ANSI_YELLOW};
use crate::core::HealPaths;
use crate::observers::{classify, run_all, ObserverReports};
use anyhow::Result;

use crate::cli::HookEvent;

pub fn run(project: &Path, event: HookEvent) -> Result<()> {
    // The hook may be invoked from settings.json on any project Claude
    // Code happens to be in — including ones that never ran `heal init`.
    // Skip silently so we don't materialise `.heal/` on a project the
    // user never opted into.
    let paths = HealPaths::new(project);
    if !paths.root().exists() {
        return Ok(());
    }
    match event {
        HookEvent::Commit => run_commit(project, &paths)?,
        HookEvent::Edit | HookEvent::Stop => {}
    }
    Ok(())
}

fn run_commit(project: &Path, paths: &HealPaths) -> Result<()> {
    // ConfigMissing means `heal init` was never run; the hook is no-op
    // in that case so an unopted project doesn't suddenly show banners.
    let cfg = match load_from_project(project) {
        Ok(c) => c,
        Err(crate::core::Error::ConfigMissing(_)) => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    let reports = run_all(project, &cfg, None, None);
    let (calibration, mut findings) = classify_with_calibration(paths, &cfg, &reports);
    // Filter the nudge through accepted findings so the per-commit
    // Critical / High counts match `heal status` instead of shaming
    // the team about findings they explicitly accepted as intrinsic.
    let accepted_map = read_accepted(&paths.findings_accepted()).unwrap_or_default();
    if !accepted_map.is_empty() {
        decorate_findings(&mut findings, &accepted_map);
    }
    // Best-effort nudge — don't fail the hook on rendering issues.
    write_nudge(
        calibration.as_ref(),
        &cfg,
        &findings,
        &mut std::io::stdout(),
    )
    .ok();
    Ok(())
}

/// Classify `reports` against the calibration on disk (if any),
/// returning both the loaded calibration and the resulting Findings.
/// `None` calibration means `heal init`/`heal calibrate` hasn't run
/// yet — the hook stays silent in that case.
pub(crate) fn classify_with_calibration(
    paths: &HealPaths,
    cfg: &Config,
    reports: &ObserverReports,
) -> (Option<Calibration>, Vec<Finding>) {
    let calibration = Calibration::load(&paths.calibration())
        .ok()
        .map(|c| c.with_overrides(cfg));
    let findings = calibration
        .as_ref()
        .map(|c| classify(reports, c, cfg))
        .unwrap_or_default();
    (calibration, findings)
}

/// Emit the one-line post-commit summary. When
/// `[features.test.coverage]` is enabled and any High/Critical
/// `coverage_pct` finding sits on a hotspot file, a second line names
/// the count so the user knows where the next test should go.
fn write_nudge(
    calibration: Option<&Calibration>,
    cfg: &Config,
    findings: &[Finding],
    out: &mut impl Write,
) -> Result<()> {
    if calibration.is_none() {
        return Ok(());
    }
    let critical = findings
        .iter()
        .filter(|f| !f.accepted && matches!(f.severity, Severity::Critical))
        .count();
    let high = findings
        .iter()
        .filter(|f| !f.accepted && matches!(f.severity, Severity::High))
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
        ansi_wrap(ANSI_CYAN, "heal status", colorize),
    )?;
    if cfg.features.test.enabled && cfg.features.test.coverage.enabled {
        let uncovered_hotspots = findings
            .iter()
            .filter(|f| {
                !f.accepted
                    && f.metric == Finding::METRIC_COVERAGE_PCT
                    && f.hotspot
                    && matches!(f.severity, Severity::High | Severity::Critical)
            })
            .count();
        if uncovered_hotspots > 0 {
            writeln!(
                out,
                "         · {}",
                ansi_wrap(
                    ANSI_YELLOW,
                    &format!("{uncovered_hotspots} uncovered hotspot"),
                    colorize,
                ),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;
    use crate::test_support::init_project_with_config;
    use tempfile::TempDir;

    /// Run the full nudge harness against an already-initialized project
    /// and return the captured `write_nudge` output. Centralised because
    /// every nudge test runs the same `run_all → classify → write_nudge`
    /// pipeline; only the calibration step and assertions vary.
    fn nudge_output(dir: &Path, paths: &HealPaths, cfg: &Config) -> String {
        let reports = run_all(dir, cfg, None, None);
        let (calibration, findings) = classify_with_calibration(paths, cfg, &reports);
        let mut buf: Vec<u8> = Vec::new();
        write_nudge(calibration.as_ref(), cfg, &findings, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn commit_runs_observers_without_writing_snapshots() {
        let dir = TempDir::new().unwrap();
        let paths = init_project_with_config(dir.path(), "fn ok() {}\n");
        // Commit hook needs `.heal/config.toml` to drive observers;
        // overwrite the default-saved config with an empty file to
        // exercise the empty-config branch in isolation.
        std::fs::write(paths.config(), "").unwrap();

        run(dir.path(), HookEvent::Commit).unwrap();
    }

    #[test]
    fn edit_is_noop() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        run(dir.path(), HookEvent::Edit).unwrap();
    }

    #[test]
    fn stop_is_noop() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        run(dir.path(), HookEvent::Stop).unwrap();
    }

    /// `write_nudge` must stay silent on a fresh project with no
    /// calibration on disk — the post-commit hook fires before the
    /// user has run `heal init`/`heal calibrate`, and we never want to
    /// pollute the commit output with a stack trace or empty banner.
    #[test]
    fn nudge_is_silent_without_calibration() {
        let dir = TempDir::new().unwrap();
        let paths = init_project_with_config(dir.path(), "fn ok(){}\n");
        let cfg = Config::default();
        let out = nudge_output(dir.path(), &paths, &cfg);
        assert!(out.is_empty(), "no calibration → no nudge, got: {out}");
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn nudge_summarises_critical_count_in_one_line() {
        // Synthesize a project where the only function trips the
        // calibration's floor, so write_nudge has something to surface.
        // A nest of `if`s drives CCN past the FLOOR_CCN=25 threshold.
        let mut src = String::from("fn busy(x: i32) -> i32 {\n");
        for _ in 0..30 {
            src.push_str("    if x > 0 { return x; }\n");
        }
        src.push_str("    0\n}\n");
        let dir = TempDir::new().unwrap();
        let paths = init_project_with_config(dir.path(), &src);
        let cfg = Config::default();
        crate::commands::calibrate::run(dir.path(), false, false).unwrap();

        let out = nudge_output(dir.path(), &paths, &cfg);
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
            out.contains("heal status"),
            "missing nudge to heal status: {out}"
        );
    }

    #[test]
    fn nudge_says_clean_when_no_critical_or_high() {
        let dir = TempDir::new().unwrap();
        let paths = init_project_with_config(dir.path(), "fn ok() {}\n");
        let cfg = Config::default();
        crate::commands::calibrate::run(dir.path(), false, false).unwrap();

        let out = nudge_output(dir.path(), &paths, &cfg);
        assert_eq!(out.trim_end(), "heal: recorded · clean");
    }
}
