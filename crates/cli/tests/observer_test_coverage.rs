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
            post_commit_refresh: None,
        },
        ..TestConfig::default()
    };
    cfg
}

#[test]
fn single_existing_lcov_path_reads_as_before() {
    // Single-package projects: one path matches, missing candidates
    // stay silent — identical output to the pre-merge behavior.
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
    assert_eq!(report.sources, vec![PathBuf::from("coverage/lcov.info")]);
}

#[test]
fn merges_every_existing_lcov_path() {
    // Polyglot monorepo: every package emits its own lcov.info and
    // every one of them must count — the old first-match-wins probe
    // silently dropped all but the first (issue #29).
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "pkg-a/coverage/lcov.info",
        "SF:pkg-a/src/a.ts\nLF:10\nLH:5\nend_of_record\n",
    );
    write(
        dir.path(),
        "pkg-b/coverage/lcov.info",
        "SF:pkg-b/src/b.ts\nLF:8\nLH:2\nend_of_record\n",
    );
    let mut cfg = cfg_test_coverage_enabled();
    cfg.features.test.coverage.lcov_paths = vec![
        "lcov.info".to_owned(), // missing — silent
        "pkg-a/coverage/lcov.info".to_owned(),
        "pkg-b/coverage/lcov.info".to_owned(),
    ];
    let report = CoverageObserver::from_config(&cfg).scan(dir.path());
    assert_eq!(report.entries.len(), 2, "both packages must survive");
    let paths: Vec<_> = report.entries.iter().map(|e| e.path.clone()).collect();
    assert!(paths.contains(&PathBuf::from("pkg-a/src/a.ts")));
    assert!(paths.contains(&PathBuf::from("pkg-b/src/b.ts")));
    assert_eq!(
        report.sources,
        vec![
            PathBuf::from("pkg-a/coverage/lcov.info"),
            PathBuf::from("pkg-b/coverage/lcov.info"),
        ],
    );
    // Back-compat: `source` is the first file that was read.
    assert_eq!(
        report.source.unwrap(),
        PathBuf::from("pkg-a/coverage/lcov.info")
    );
}

#[test]
fn resolves_package_relative_sf_paths() {
    // vitest / jest / scoverage run from the package root and emit
    // `SF:` paths relative to the package, not the repo. The resolver
    // probes the lcov file's ancestor directories for a path that
    // exists on disk.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "pkg-a/src/foo.ts", "export const x = 1;\n");
    write(
        dir.path(),
        "pkg-a/coverage/lcov.info",
        "SF:src/foo.ts\nLF:4\nLH:1\nend_of_record\n",
    );
    let mut cfg = cfg_test_coverage_enabled();
    cfg.features.test.coverage.lcov_paths = vec!["pkg-a/coverage/lcov.info".to_owned()];
    let report = CoverageObserver::from_config(&cfg).scan(dir.path());
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].path, PathBuf::from("pkg-a/src/foo.ts"));
}

#[test]
fn colliding_entries_across_files_max_merge() {
    // A hand-merged root lcov.info can coexist with the per-package
    // file it was built from; counters max-merge (same rule as
    // duplicate `SF` records within one file) instead of aliasing or
    // double-counting.
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "lcov.info",
        "SF:pkg-a/src/a.ts\nLF:10\nLH:4\nend_of_record\n",
    );
    write(
        dir.path(),
        "pkg-a/coverage/lcov.info",
        "SF:pkg-a/src/a.ts\nLF:10\nLH:7\nend_of_record\n",
    );
    let mut cfg = cfg_test_coverage_enabled();
    cfg.features.test.coverage.lcov_paths = vec![
        "lcov.info".to_owned(),
        "pkg-a/coverage/lcov.info".to_owned(),
    ];
    let report = CoverageObserver::from_config(&cfg).scan(dir.path());
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].lines_hit, 7);
    assert!((report.entries[0].line_coverage_pct - 70.0).abs() < 1e-9);
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
        sources: Vec::new(),
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
