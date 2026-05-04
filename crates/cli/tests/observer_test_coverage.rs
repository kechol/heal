//! Integration coverage for `[features.test.coverage]`: lcov ingestion
//! against tempdir layouts, `CoverageReport` lookup helpers, and
//! severity classification through `Feature::lower`.

use std::path::PathBuf;

use heal_cli::core::config::{Config, TestConfig, TestCoverageConfig};
use heal_cli::core::finding::{Finding, IntoFindings};
use heal_cli::observer::test::coverage::{CoverageObserver, CoverageReport};

mod common;
use common::write;

fn cfg_test_coverage_enabled() -> Config {
    let mut cfg = Config::default();
    cfg.features.test = TestConfig {
        enabled: true,
        coverage: TestCoverageConfig {
            enabled: true,
            lcov_paths: vec!["lcov.info".to_owned()],
        },
        ..TestConfig::default()
    };
    cfg
}

#[test]
fn picks_first_existing_lcov_path() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "coverage/lcov.info",
        "SF:src/lib.rs\nLF:1\nLH:0\nend_of_record\n",
    );
    let mut cfg = cfg_test_coverage_enabled();
    cfg.features.test.coverage.lcov_paths = vec![
        "missing.info".to_owned(),
        "coverage/lcov.info".to_owned(),
        "lcov.info".to_owned(),
    ];
    let report = CoverageObserver::from_config(&cfg).scan(dir.path());
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.source.unwrap(), PathBuf::from("coverage/lcov.info"));
}

#[test]
fn returns_empty_when_no_lcov_file_present() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = cfg_test_coverage_enabled();
    let report = CoverageObserver::from_config(&cfg).scan(dir.path());
    assert!(report.entries.is_empty());
    assert!(report.source.is_none());
}

#[test]
fn returns_empty_when_feature_disabled() {
    // Feature off but a stray lcov.info present — observer must still
    // be a no-op so projects don't get implicit coverage findings.
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "lcov.info",
        "SF:src/lib.rs\nLF:1\nLH:0\nend_of_record\n",
    );
    let cfg = Config::default();
    let report = CoverageObserver::from_config(&cfg).scan(dir.path());
    assert!(report.entries.is_empty());
}

#[test]
fn into_findings_skips_fully_covered_files() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "lcov.info",
        "\
SF:src/full.rs
LF:5
LH:5
end_of_record
SF:src/half.rs
LF:4
LH:2
end_of_record
",
    );
    let cfg = cfg_test_coverage_enabled();
    let report = CoverageObserver::from_config(&cfg).scan(dir.path());
    let findings = report.into_findings();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].metric, Finding::METRIC_COVERAGE_PCT);
    assert_eq!(findings[0].location.file, PathBuf::from("src/half.rs"));
    assert!(findings[0].summary.contains("Coverage="));
    // The summary's "Coverage=N%" form is what `metric_value` and
    // `short_label` recover the integer percentage from.
    assert_eq!(findings[0].metric_value(), Some(50.0));
}

#[test]
fn ratio_for_returns_coverage_ratio() {
    let report = CoverageReport {
        source: None,
        entries: vec![heal_cli::observer::test::coverage::CoverageEntry {
            path: PathBuf::from("src/lib.rs"),
            lines_found: 10,
            lines_hit: 8,
            branches_found: 0,
            branches_hit: 0,
            line_coverage_pct: 80.0,
        }],
    };
    let r = report
        .ratio_for(std::path::Path::new("src/lib.rs"))
        .unwrap();
    assert!((r - 0.8).abs() < 1e-9);
}
