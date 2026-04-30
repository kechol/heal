//! `heal calibrate` — recalibrate codebase-relative Severity thresholds.
//!
//! Two paths:
//!   - `heal calibrate [--reason <text>]` rescans every observer, rewrites
//!     `.heal/calibration.toml`, and appends a `CalibrationEvent` to
//!     `.heal/snapshots/`.
//!   - `heal calibrate --check` reads the current calibration and the
//!     latest snapshot, evaluates the auto-detect triggers
//!     ([`RecalibrationCheck`]), and prints a recommendation. No files
//!     are written.
//!
//! HEAL never recalibrates automatically (TODO §「ユーザー提案のみで
//! 自動再較正はしない」); the post-commit nudge will surface
//! `--check` results, but the user must invoke `heal calibrate`
//! themselves.

use std::io::IsTerminal;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::core::calibration::{Calibration, RecalibrationCheck};
use crate::core::config::load_from_project;
use crate::core::eventlog::{Event, EventLog};
use crate::core::snapshot::{ansi_wrap, ANSI_GREEN, ANSI_YELLOW};
use crate::core::HealPaths;
use crate::observers::{build_calibration, run_all};

pub fn run(project: &Path, reason: Option<String>, check_only: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    let cfg = load_from_project(project).with_context(|| {
        format!(
            "loading {} (run `heal init` first?)",
            paths.config().display(),
        )
    })?;

    if check_only {
        run_check(&paths);
        return Ok(());
    }

    let reason = reason.unwrap_or_else(|| "manual".to_owned());
    let reports = run_all(project, &cfg, None);
    let calibration = build_calibration(&reports, &cfg);
    calibration.save(&paths.calibration())?;

    let event = calibration.to_event(reason.clone());
    let payload =
        serde_json::to_value(&event).expect("CalibrationEvent serialization is infallible");
    EventLog::new(paths.snapshots_dir()).append(&Event::new("calibrate", payload))?;

    println!("Recalibrated {} ({reason})", paths.calibration().display());
    println!("  codebase_files: {}", calibration.meta.codebase_files);
    if let Some(c) = calibration.calibration.ccn.as_ref() {
        println!("  ccn p95:        {:.1}", c.p95);
    }
    if let Some(c) = calibration.calibration.cognitive.as_ref() {
        println!("  cognitive p95:  {:.1}", c.p95);
    }
    if let Some(c) = calibration.calibration.hotspot.as_ref() {
        println!("  hotspot p90:    {:.1}", c.p90);
    }
    Ok(())
}

fn run_check(paths: &HealPaths) {
    let calibration_path = paths.calibration();
    let Ok(calibration) = Calibration::load(&calibration_path) else {
        println!(
            "no calibration at {} — run `heal init` or `heal calibrate` to create one",
            calibration_path.display(),
        );
        return;
    };
    let snapshots = EventLog::new(paths.snapshots_dir());
    let check = RecalibrationCheck::evaluate(&snapshots, &calibration, Utc::now());
    let colorize = std::io::stdout().is_terminal();

    if !check.fired() {
        println!(
            "{} no recalibration triggers fired (last calibration: {})",
            ansi_wrap(ANSI_GREEN, "OK", colorize),
            calibration.meta.created_at.format("%Y-%m-%d"),
        );
        return;
    }

    println!(
        "{}: recalibration triggers fired",
        ansi_wrap(ANSI_YELLOW, "recommended", colorize),
    );
    if let Some(days) = check.age_exceeded_days {
        println!("  - calibration is {days} days old (>90)");
    }
    if let Some(pct) = check.file_count_delta_pct {
        println!(
            "  - codebase size changed by {:+.0}% since last calibration",
            pct * 100.0
        );
    }
    if let Some(streak) = check.critical_clean_streak_days {
        println!("  - {streak} days of [critical] = 0 (>=30) — thresholds may be too lenient");
    }
    println!("Run `heal calibrate --reason \"<note>\"` to refresh.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::calibration::CalibrationEvent;
    use crate::core::config::Config;
    use crate::core::eventlog::EventLog as EvLog;
    use crate::core::snapshot::MetricsSnapshot;
    use crate::test_support::{commit, init_repo};
    use tempfile::TempDir;

    fn init_project(dir: &Path) {
        init_repo(dir);
        commit(dir, "main.rs", "fn main() {}\n", "solo@example.com", "init");
        let paths = HealPaths::new(dir);
        paths.ensure().unwrap();
        Config::default().save(&paths.config()).unwrap();
    }

    #[test]
    fn calibrate_writes_calibration_toml_and_event() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        let paths = HealPaths::new(dir.path());

        run(dir.path(), Some("test-run".into()), false).unwrap();

        assert!(
            paths.calibration().exists(),
            "calibration.toml must be written"
        );

        // The snapshots/ dir must contain a `calibrate` event.
        let log = EvLog::new(paths.snapshots_dir());
        let events: Vec<_> = log.try_iter().unwrap().filter_map(Result::ok).collect();
        let calibrate_evs: Vec<_> = events.iter().filter(|e| e.event == "calibrate").collect();
        assert_eq!(
            calibrate_evs.len(),
            1,
            "exactly one calibrate event expected"
        );
        let ev: CalibrationEvent = serde_json::from_value(calibrate_evs[0].data.clone()).unwrap();
        assert_eq!(ev.reason, "test-run");
    }

    #[test]
    fn calibrate_check_returns_quiet_for_fresh_project() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        let paths = HealPaths::new(dir.path());
        // First calibrate so a calibration.toml exists, then immediately --check.
        run(dir.path(), None, false).unwrap();

        // Append a fresh commit-like snapshot so `latest_in_segments`
        // resolves; mimic what `heal hook commit` would write. critical=0,
        // codebase_files matches calibration.
        let snap = MetricsSnapshot {
            severity_counts: Some(crate::core::snapshot::SeverityCounts::default()),
            codebase_files: Some(1),
            ..MetricsSnapshot::default()
        };
        EvLog::new(paths.snapshots_dir())
            .append(&Event::new("commit", serde_json::to_value(&snap).unwrap()))
            .unwrap();

        run(dir.path(), None, true).unwrap();
    }

    #[test]
    fn calibrate_check_without_calibration_prints_hint() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        // No calibration written yet — --check must succeed gracefully.
        run(dir.path(), None, true).unwrap();
    }
}
