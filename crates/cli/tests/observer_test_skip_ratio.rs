//! Integration coverage for `[features.test]` `skip_ratio`. Drives the
//! observer over a temp project tree with Rust + Python + JS test files
//! and asserts the per-file Findings, severity classification (via the
//! fallback calibration), and the `FeatureRegistry` wiring all line up.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use heal_cli::core::calibration::{
    Calibration, CalibrationMeta, MetricCalibrations, STRATEGY_PERCENTILE,
};
use heal_cli::core::config::{Config, TestConfig};
use heal_cli::core::finding::Finding;
use heal_cli::core::severity::Severity;
use heal_cli::feature::FeatureRegistry;
use heal_cli::observer::test::skip_ratio::{SkipRatioObserver, SkipRatioReport};
use heal_cli::observers::ObserverReports;

fn cfg_test_enabled() -> Config {
    let mut cfg = Config::default();
    cfg.features.test = TestConfig {
        enabled: true,
        ..TestConfig::default()
    };
    cfg
}

fn empty_reports() -> ObserverReports {
    ObserverReports::default()
}

#[test]
fn observer_walks_test_paths_and_emits_per_language_entries() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests/foo_test.rs"),
        "#[test] fn a() {} #[test] fn b() {} #[test] #[ignore] fn c() {}",
    )
    .unwrap();

    fs::create_dir_all(root.join("python_tests")).unwrap();
    fs::write(
        root.join("python_tests/test_app.py"),
        "import pytest\n\
         def test_one(): pass\n\
         @pytest.mark.skip\n\
         def test_two(): pass\n",
    )
    .unwrap();

    fs::create_dir_all(root.join("js")).unwrap();
    fs::write(
        root.join("js/foo.test.ts"),
        "describe('grp', () => {\n\
           it('a', () => {});\n\
           it.skip('b', () => {});\n\
         });\n",
    )
    .unwrap();

    let mut cfg = cfg_test_enabled();
    // Override defaults so the python_tests/ directory is also included.
    cfg.features.test.test_paths = vec![
        "tests/**".into(),
        "python_tests/**".into(),
        "**/*.test.ts".into(),
    ];

    let report = SkipRatioObserver::from_config(&cfg).scan(root);
    assert_eq!(
        report.entries.len(),
        3,
        "expected one entry per test file, got: {:#?}",
        report.entries,
    );

    let by_path: BTreeMap<_, _> = report.entries.iter().map(|e| (e.path.clone(), e)).collect();
    let rs = by_path
        .get(&PathBuf::from("tests/foo_test.rs"))
        .expect("rust test file present");
    assert_eq!(rs.total_tests, 3);
    assert_eq!(rs.skipped_tests, 1);

    let py = by_path
        .get(&PathBuf::from("python_tests/test_app.py"))
        .expect("python test file present");
    assert_eq!(py.total_tests, 2);
    assert_eq!(py.skipped_tests, 1);

    let ts = by_path
        .get(&PathBuf::from("js/foo.test.ts"))
        .expect("typescript test file present");
    assert_eq!(ts.total_tests, 3);
    assert_eq!(ts.skipped_tests, 1);
}

#[test]
fn lower_all_classifies_skip_ratio_against_fallback_calibration() {
    let report = SkipRatioReport {
        entries: vec![heal_cli::observer::test::skip_ratio::SkipRatioEntry {
            path: PathBuf::from("tests/dirty.rs"),
            language: "rust".into(),
            total_tests: 4,
            skipped_tests: 1,
            skip_pct: 25.0,
        }],
    };
    let mut reports = empty_reports();
    reports.skip_ratio = Some(report);
    let cfg = cfg_test_enabled();
    let cal = Calibration {
        meta: CalibrationMeta {
            created_at: chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap_or_default(),
            codebase_files: 1,
            strategy: STRATEGY_PERCENTILE.to_owned(),
            calibrated_at_sha: None,
        },
        // No skip_ratio entry → observer falls back to FALLBACK_CALIBRATION.
        calibration: MetricCalibrations::default(),
        workspaces: BTreeMap::new(),
    };

    let registry = FeatureRegistry::builtin();
    let findings = registry.lower_all(&reports, &cfg, &cal);

    let skip_finding = findings
        .iter()
        .find(|f| f.metric == Finding::METRIC_SKIP_RATIO)
        .expect("skip_ratio finding present");
    // 25% > floor_critical (20%) → Critical.
    assert_eq!(skip_finding.severity, Severity::Critical);
    assert!(
        skip_finding.summary.starts_with("Skip=25%"),
        "summary should carry the rounded percentage, got: {}",
        skip_finding.summary,
    );
}

#[test]
fn lower_all_emits_no_findings_when_feature_disabled() {
    let report = SkipRatioReport {
        entries: vec![heal_cli::observer::test::skip_ratio::SkipRatioEntry {
            path: PathBuf::from("tests/dirty.rs"),
            language: "rust".into(),
            total_tests: 4,
            skipped_tests: 1,
            skip_pct: 25.0,
        }],
    };
    let mut reports = empty_reports();
    reports.skip_ratio = Some(report);
    let cfg = Config::default(); // features.test disabled
    let cal = Calibration::default();

    let findings = FeatureRegistry::builtin().lower_all(&reports, &cfg, &cal);
    assert!(
        findings
            .iter()
            .all(|f| f.metric != Finding::METRIC_SKIP_RATIO),
        "no skip_ratio findings should emit when [features.test] is off",
    );
}
