//! `heal calibrate` — calibrate codebase-relative Severity thresholds.
//!
//! Behavior:
//!   - `calibration.toml` missing, or `--force`: rescan every observer
//!     and rewrite `.heal/calibration.toml`. The new file carries
//!     `meta.calibrated_at_sha` and `meta.codebase_files` so the
//!     `heal-config` skill can later judge drift without consulting any
//!     event log.
//!   - `calibration.toml` present (no `--force`): print the freshness
//!     summary and point at `heal calibrate --force` as the way to
//!     refresh. The `heal-config` skill is responsible for deciding
//!     whether to suggest a recalibration; HEAL itself never auto-fires.
//!
//! HEAL never recalibrates automatically (TODO §「ユーザー提案のみで
//! 自動再較正はしない」). The user (or skill on the user's behalf)
//! always invokes `heal calibrate --force` themselves.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::core::calibration::Calibration;
use crate::core::config::load_from_project;
use crate::core::HealPaths;
use crate::observers::{build_calibration, run_all};

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
        run_status(&paths, as_json);
        return Ok(());
    }

    let reports = run_all(project, &cfg, None, None);
    let calibration = build_calibration(project, &reports, &cfg);
    calibration.save(&calibration_path)?;

    if as_json {
        super::emit_json(&CalibrateReport {
            kind: "recalibrated",
            path: calibration_path.display().to_string(),
            calibration: Some(&calibration),
        });
        return Ok(());
    }

    println!("Recalibrated {}", calibration_path.display());
    println!("  codebase_files: {}", calibration.meta.codebase_files);
    if let Some(sha) = calibration.meta.calibrated_at_sha.as_deref() {
        println!("  calibrated_at:  {}", &sha[..sha.len().min(12)]);
    }
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

fn run_status(paths: &HealPaths, as_json: bool) {
    let calibration_path = paths.calibration();
    let Ok(calibration) = Calibration::load(&calibration_path) else {
        // The path-exists guard above already filtered this out; if a
        // race deleted the file we still want to fail soft and prompt.
        if as_json {
            super::emit_json(&CalibrateReport {
                kind: "missing",
                path: calibration_path.display().to_string(),
                calibration: None,
            });
        } else {
            println!(
                "no calibration at {} — re-run `heal calibrate --force` to create one",
                calibration_path.display(),
            );
        }
        return;
    };

    if as_json {
        super::emit_json(&CalibrateReport {
            kind: "ok",
            path: calibration_path.display().to_string(),
            calibration: Some(&calibration),
        });
        return;
    }

    println!(
        "calibration present at {} (created {})",
        calibration_path.display(),
        calibration.meta.created_at.format("%Y-%m-%d"),
    );
    if let Some(sha) = calibration.meta.calibrated_at_sha.as_deref() {
        println!("  calibrated_at_sha:    {}", &sha[..sha.len().min(12)]);
    }
    println!(
        "  calibrated_at_files:  {}",
        calibration.meta.codebase_files,
    );
    println!(
        "Run `heal calibrate --force` to rebuild the percentile breaks from the current codebase."
    );
}

/// Stable JSON contract for `heal calibrate --json`. `kind` distinguishes
/// the three reachable states so callers can branch without parsing prose.
#[derive(Debug, Serialize)]
struct CalibrateReport<'a> {
    /// One of `recalibrated`, `ok`, `missing`.
    kind: &'static str,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    calibration: Option<&'a Calibration>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;
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
    fn calibrate_writes_calibration_toml_when_missing() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        let paths = HealPaths::new(dir.path());

        run(dir.path(), false, false).unwrap();

        assert!(
            paths.calibration().exists(),
            "calibration.toml must be written"
        );
        let calibration = Calibration::load(&paths.calibration()).unwrap();
        assert!(
            calibration.meta.calibrated_at_sha.is_some(),
            "calibrated_at_sha must be captured from HEAD"
        );
    }

    #[test]
    fn calibrate_default_does_not_rewrite_when_calibration_exists() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        let paths = HealPaths::new(dir.path());

        run(dir.path(), true, false).unwrap();
        let mtime_before = std::fs::metadata(paths.calibration())
            .unwrap()
            .modified()
            .unwrap();

        run(dir.path(), false, false).unwrap();
        let mtime_after = std::fs::metadata(paths.calibration())
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(
            mtime_before, mtime_after,
            "default run must not rewrite calibration.toml"
        );
    }

    #[test]
    fn calibrate_force_rewrites_existing_calibration() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        let paths = HealPaths::new(dir.path());

        run(dir.path(), false, false).unwrap();
        let first_created = Calibration::load(&paths.calibration())
            .unwrap()
            .meta
            .created_at;

        // Sleep is unnecessary — `chrono::Utc::now()` advances on each call.
        run(dir.path(), true, false).unwrap();
        let second_created = Calibration::load(&paths.calibration())
            .unwrap()
            .meta
            .created_at;

        assert!(
            second_created >= first_created,
            "--force must produce a fresh calibration"
        );
    }
}
