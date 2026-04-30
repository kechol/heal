//! `heal calibrate` — calibrate codebase-relative Severity thresholds.
//!
//! Behavior:
//!   - `calibration.toml` missing, or `--force`: rescan every observer,
//!     rewrite `.heal/calibration.toml`, and append a `CalibrationEvent`
//!     to `.heal/snapshots/`.
//!   - `calibration.toml` present (no `--force`): read it and evaluate
//!     the auto-detect triggers ([`RecalibrationCheck`]); print a
//!     recommendation and surface `--force` as the way to refresh. No
//!     files are written.
//!
//! HEAL never recalibrates automatically (TODO §「ユーザー提案のみで
//! 自動再較正はしない」); the post-commit nudge will surface
//! recalibration drift, but the user must invoke `heal calibrate
//! --force` themselves.

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

/// Reason recorded on every audit-log entry. The free-form `--reason`
/// flag was removed (it was overkill for v0.1's manual flow); a single
/// constant keeps `CalibrationEvent`'s schema stable for any external
/// reader without forcing them to handle `Option<String>`.
const CALIBRATE_REASON: &str = "manual";

pub fn run(project: &Path, force: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    let cfg = load_from_project(project).with_context(|| {
        format!(
            "loading {} (run `heal init` first?)",
            paths.config().display(),
        )
    })?;

    let calibration_path = paths.calibration();
    if !force && calibration_path.exists() {
        run_check(&paths);
        return Ok(());
    }

    let reports = run_all(project, &cfg, None);
    let calibration = build_calibration(&reports, &cfg);
    calibration.save(&calibration_path)?;

    let event = calibration.to_event(CALIBRATE_REASON.to_owned());
    let payload =
        serde_json::to_value(&event).expect("CalibrationEvent serialization is infallible");
    EventLog::new(paths.snapshots_dir()).append(&Event::new("calibrate", payload))?;

    println!("Recalibrated {}", calibration_path.display());
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
        // The path-exists guard above already filtered this out; if a
        // race deleted the file we still want to fail soft and prompt.
        println!(
            "no calibration at {} — re-run `heal calibrate --force` to create one",
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
        println!("Run `heal calibrate --force` to refresh anyway.");
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
    println!("Run `heal calibrate --force` to refresh.");
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
    fn calibrate_writes_calibration_toml_and_event_when_missing() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        let paths = HealPaths::new(dir.path());

        // No calibration file yet — default run should generate it.
        run(dir.path(), false).unwrap();

        assert!(
            paths.calibration().exists(),
            "calibration.toml must be written"
        );

        let log = EvLog::new(paths.snapshots_dir());
        let events: Vec<_> = log.try_iter().unwrap().filter_map(Result::ok).collect();
        let calibrate_evs: Vec<_> = events.iter().filter(|e| e.event == "calibrate").collect();
        assert_eq!(
            calibrate_evs.len(),
            1,
            "exactly one calibrate event expected"
        );
        let ev: CalibrationEvent = serde_json::from_value(calibrate_evs[0].data.clone()).unwrap();
        assert_eq!(ev.reason, CALIBRATE_REASON);
    }

    #[test]
    fn calibrate_default_runs_check_when_calibration_exists() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        let paths = HealPaths::new(dir.path());

        // Seed a calibration via --force so the second invocation hits
        // the read-only check path.
        run(dir.path(), true).unwrap();

        // Append a fresh commit-like snapshot so `latest_in_segments`
        // resolves; mimic what `heal hook commit` would write.
        let snap = MetricsSnapshot {
            severity_counts: Some(crate::core::snapshot::SeverityCounts::default()),
            codebase_files: Some(1),
            ..MetricsSnapshot::default()
        };
        EvLog::new(paths.snapshots_dir())
            .append(&Event::new("commit", serde_json::to_value(&snap).unwrap()))
            .unwrap();

        let events_before = EvLog::new(paths.snapshots_dir())
            .try_iter()
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.event == "calibrate")
            .count();

        // Default run: must NOT append a new calibrate event.
        run(dir.path(), false).unwrap();

        let events_after = EvLog::new(paths.snapshots_dir())
            .try_iter()
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.event == "calibrate")
            .count();
        assert_eq!(
            events_before, events_after,
            "default run must not write when calibration.toml exists"
        );
    }

    #[test]
    fn calibrate_force_overwrites_existing_calibration() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        let paths = HealPaths::new(dir.path());

        run(dir.path(), false).unwrap();
        let mtime_first = std::fs::metadata(paths.calibration())
            .unwrap()
            .modified()
            .unwrap();

        // Sleep is unnecessary on most filesystems but the assertion is
        // about a fresh calibrate event being recorded, which is robust.
        run(dir.path(), true).unwrap();

        let calibrate_count = EvLog::new(paths.snapshots_dir())
            .try_iter()
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.event == "calibrate")
            .count();
        assert_eq!(
            calibrate_count, 2,
            "--force on an existing calibration must append a new event"
        );
        // Touch-check: even if mtime equals (rare on coarse FS), the
        // event count assertion above is the load-bearing check.
        let _ = mtime_first;
    }
}
