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
use serde::Serialize;

use crate::core::calibration::{Calibration, RecalibrationCheck};
use crate::core::config::load_from_project;
use crate::core::eventlog::{Event, EventLog};
use crate::core::snapshot::{ansi_wrap, ANSI_GREEN, ANSI_YELLOW};
use crate::core::HealPaths;
use crate::observers::{build_calibration, run_all};

/// Reason field on every `CalibrationEvent`. A single value keeps the
/// audit-log schema stable for external readers.
const CALIBRATE_REASON: &str = "manual";

pub fn run(project: &Path, force: bool, as_json: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    let cfg = load_from_project(project).with_context(|| {
        format!(
            "loading {} (run `heal init` first?)",
            paths.config().display(),
        )
    })?;

    let calibration_path = paths.calibration();
    if !force && calibration_path.exists() {
        run_check(&paths, as_json);
        return Ok(());
    }

    let reports = run_all(project, &cfg, None);
    let calibration = build_calibration(&reports, &cfg);
    calibration.save(&calibration_path)?;

    let event = calibration.to_event(CALIBRATE_REASON.to_owned());
    let payload =
        serde_json::to_value(&event).expect("CalibrationEvent serialization is infallible");
    EventLog::new(paths.snapshots_dir()).append(&Event::new("calibrate", payload))?;

    if as_json {
        super::emit_json(&CalibrateReport {
            kind: "recalibrated",
            path: calibration_path.display().to_string(),
            calibration: Some(&calibration),
            recalibration_check: None,
        });
        return Ok(());
    }

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

fn run_check(paths: &HealPaths, as_json: bool) {
    let calibration_path = paths.calibration();
    let Ok(calibration) = Calibration::load(&calibration_path) else {
        // The path-exists guard above already filtered this out; if a
        // race deleted the file we still want to fail soft and prompt.
        if as_json {
            super::emit_json(&CalibrateReport {
                kind: "missing",
                path: calibration_path.display().to_string(),
                calibration: None,
                recalibration_check: None,
            });
        } else {
            println!(
                "no calibration at {} — re-run `heal calibrate --force` to create one",
                calibration_path.display(),
            );
        }
        return;
    };
    let snapshots = EventLog::new(paths.snapshots_dir());
    let check = RecalibrationCheck::evaluate(&snapshots, &calibration, Utc::now());

    if as_json {
        super::emit_json(&CalibrateReport {
            kind: if check.fired() {
                "recalibration_recommended"
            } else {
                "ok"
            },
            path: calibration_path.display().to_string(),
            calibration: Some(&calibration),
            recalibration_check: Some(RecalibrationCheckJson::from(&check)),
        });
        return;
    }

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
        println!(
            "  - {streak} days of [critical] = 0 (>=30) — codebase may have graduated below \
             proxy-metric floors, or thresholds may be too lenient"
        );
    }
    println!("Run `heal calibrate --force` to refresh.");
}

/// Stable JSON contract for `heal calibrate --json`. `kind` distinguishes
/// the four reachable states so callers can branch without parsing prose.
#[derive(Debug, Serialize)]
struct CalibrateReport<'a> {
    /// One of `recalibrated`, `ok`, `recalibration_recommended`, `missing`.
    kind: &'static str,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    calibration: Option<&'a Calibration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recalibration_check: Option<RecalibrationCheckJson>,
}

#[derive(Debug, Serialize)]
struct RecalibrationCheckJson {
    fired: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    age_exceeded_days: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_count_delta_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    critical_clean_streak_days: Option<i64>,
}

impl From<&RecalibrationCheck> for RecalibrationCheckJson {
    fn from(c: &RecalibrationCheck) -> Self {
        Self {
            fired: c.fired(),
            age_exceeded_days: c.age_exceeded_days,
            file_count_delta_pct: c.file_count_delta_pct,
            critical_clean_streak_days: c.critical_clean_streak_days,
        }
    }
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
        run(dir.path(), false, false).unwrap();

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
        run(dir.path(), true, false).unwrap();

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
        run(dir.path(), false, false).unwrap();

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

        run(dir.path(), false, false).unwrap();
        run(dir.path(), true, false).unwrap();

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
    }
}
