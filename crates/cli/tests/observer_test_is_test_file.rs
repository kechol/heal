//! Coverage for the post-classify `is_test_file` tagging pass driven
//! by `[features.test].test_paths`. Asserts the flag flips on findings
//! whose primary file matches the test glob set when the feature is
//! on, stays `false` when the feature is off, and round-trips
//! through JSON.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use heal_cli::core::calibration::{
    Calibration, CalibrationMeta, MetricCalibration, MetricCalibrations, STRATEGY_PERCENTILE,
};
use heal_cli::core::config::{Config, TestConfig};
use heal_cli::core::finding::{Finding, Location};
use heal_cli::core::severity::Severity;
use heal_cli::feature::FeatureRegistry;
use heal_cli::observer::code::change_coupling::{ChangeCouplingReport, FilePair, PairClass};
use heal_cli::observers::ObserverReports;

fn finding(path: &str) -> Finding {
    Finding::new(
        "ccn",
        Location {
            file: PathBuf::from(path),
            line: Some(1),
            symbol: Some("fn".into()),
        },
        format!("CCN=10 in {path}"),
        path,
    )
}

#[test]
fn finding_default_is_test_file_is_false() {
    let f = finding("src/lib.rs");
    assert!(!f.is_test_file);
}

#[test]
fn config_test_paths_default_covers_common_conventions() {
    let cfg = TestConfig::default();
    let want = [
        "tests/**",
        "**/*_test.rs",
        "**/*.test.ts",
        "**/*_test.go",
        "**/test_*.py",
        "**/*Test.scala",
    ];
    for pat in want {
        assert!(
            cfg.test_paths.iter().any(|p| p == pat),
            "default test_paths missing {pat}: {:?}",
            cfg.test_paths,
        );
    }
}

#[test]
fn matcher_honours_test_paths_globs() {
    use heal_cli::observer::shared::walk::ExcludeMatcher;
    let m = ExcludeMatcher::compile(
        Path::new(""),
        &["tests/**".to_owned(), "**/*_test.rs".to_owned()],
    )
    .unwrap();
    assert!(m.is_excluded(Path::new("tests/foo.rs"), false));
    assert!(m.is_excluded(Path::new("crates/cli/src/lib_test.rs"), false));
    assert!(!m.is_excluded(Path::new("src/lib.rs"), false));
}

#[test]
fn finding_with_is_test_file_round_trips_through_json() {
    let mut f = finding("tests/foo.rs");
    f.is_test_file = true;
    f.severity = Severity::Medium;
    let json = serde_json::to_string(&f).unwrap();
    assert!(json.contains("\"is_test_file\":true"));
    let back: Finding = serde_json::from_str(&json).unwrap();
    assert!(back.is_test_file);
}

#[test]
fn finding_with_default_is_test_file_omits_field_in_json() {
    // `skip_serializing_if = is_false` — non-test findings stay byte-
    // identical to v3 cache files for projects that don't opt into
    // `[features.test]`.
    let f = finding("src/lib.rs");
    let json = serde_json::to_string(&f).unwrap();
    assert!(
        !json.contains("is_test_file"),
        "non-test finding should omit the field, got {json}"
    );
}

/// End-to-end: route findings through `FeatureRegistry::lower_all`
/// with one finding under `tests/` and one under `src/` and assert
/// the post-pass flips `is_test_file` correctly.
fn pair(a: &str, b: &str, count: u32, class: PairClass) -> FilePair {
    let (a_p, b_p) = if a < b { (a, b) } else { (b, a) };
    FilePair {
        a: PathBuf::from(a_p),
        b: PathBuf::from(b_p),
        count,
        direction: None,
        class: Some(class),
    }
}

fn observer_reports_with_pairs(pairs: Vec<FilePair>) -> ObserverReports {
    ObserverReports {
        loc: heal_cli::observer::code::loc::LocReport::default(),
        complexity: heal_cli::observer::code::complexity::ComplexityReport::default(),
        complexity_observer: heal_cli::observer::code::complexity::ComplexityObserver::default(),
        churn: None,
        change_coupling: Some(ChangeCouplingReport {
            pairs,
            ..Default::default()
        }),
        duplication: None,
        hotspot: None,
        lcom: None,
        doc_pairs: None,
        doc_freshness: None,
        doc_drift: None,
        doc_coverage: None,
        doc_link_health: None,
        orphan_pages: None,
        todo_density: None,
        coverage: None,
        skip_ratio: None,
    }
}

fn calibration_with_p50(p50: f64) -> Calibration {
    Calibration {
        meta: CalibrationMeta {
            created_at: chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap_or_default(),
            codebase_files: 1,
            strategy: STRATEGY_PERCENTILE.to_owned(),
            calibrated_at_sha: None,
        },
        calibration: MetricCalibrations {
            change_coupling: Some(MetricCalibration {
                p50,
                p75: p50 * 2.0,
                p90: p50 * 3.0,
                p95: p50 * 4.0,
                floor_critical: None,
                floor_ok: None,
            }),
            ..MetricCalibrations::default()
        },
        workspaces: BTreeMap::new(),
    }
}

#[test]
fn lower_all_tags_findings_under_test_paths() {
    // Two Genuine pairs: one between two test files, one between two
    // production files. Genuine bypasses the TestSrc demote branch in
    // change_coupling::lower, so both pairs emit real findings. The
    // post-pass should flip `is_test_file` only on the test-side
    // finding.
    let pairs = vec![
        pair("tests/foo.rs", "tests/bar.rs", 10, PairClass::Genuine),
        pair("src/foo.rs", "src/bar.rs", 10, PairClass::Genuine),
    ];
    let reports = observer_reports_with_pairs(pairs);
    let mut cfg = Config::default();
    cfg.features.test = TestConfig {
        enabled: true,
        ..TestConfig::default()
    };
    let cal = calibration_with_p50(2.0);

    let registry = FeatureRegistry::builtin();
    let findings = registry.lower_all(&reports, &cfg, &cal);

    // The change_coupling Feature emits one finding per surviving
    // pair. Each carries the lex-smaller path as `location.file`.
    let test_finding = findings
        .iter()
        .find(|f| f.location.file == Path::new("tests/bar.rs"))
        .expect("test-side finding present");
    let prod_finding = findings
        .iter()
        .find(|f| f.location.file == Path::new("src/bar.rs"))
        .expect("production-side finding present");

    assert!(
        test_finding.is_test_file,
        "expected test finding to be tagged is_test_file=true, got: {test_finding:#?}"
    );
    assert!(
        !prod_finding.is_test_file,
        "expected production finding to stay is_test_file=false, got: {prod_finding:#?}"
    );
}

#[test]
fn lower_all_does_not_tag_when_test_feature_disabled() {
    let pairs = vec![pair("tests/foo.rs", "tests/bar.rs", 10, PairClass::Genuine)];
    let reports = observer_reports_with_pairs(pairs);
    let cfg = Config::default();
    // features.test stays at default (disabled).
    let cal = calibration_with_p50(2.0);

    let registry = FeatureRegistry::builtin();
    let findings = registry.lower_all(&reports, &cfg, &cal);

    assert!(
        findings.iter().all(|f| !f.is_test_file),
        "no finding should carry is_test_file=true when feature is disabled, got: {findings:#?}",
    );
}
