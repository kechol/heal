//! Coverage for the `change_coupling.drift` submetric introduced
//! alongside `[features.test]`. A `TestSrc` pair whose joint count
//! sits below the project's `change_coupling.p50` is retagged as
//! `change_coupling.drift` (`Severity::Medium`, real Finding)
//! instead of `change_coupling.expected` (Advisory). The retag
//! fires only when the test feature is enabled — disabled feature
//! keeps the existing `expected` demotion.

use std::collections::BTreeMap;
use std::path::PathBuf;

use heal_cli::core::calibration::{
    Calibration, CalibrationMeta, MetricCalibration, MetricCalibrations, STRATEGY_PERCENTILE,
};
use heal_cli::core::config::{Config, TestConfig};
use heal_cli::core::finding::Finding;
use heal_cli::core::severity::Severity;
use heal_cli::feature::FeatureRegistry;
use heal_cli::observer::code::change_coupling::{ChangeCouplingReport, FilePair, PairClass};
use heal_cli::observers::ObserverReports;

fn classified_pair(a: &str, b: &str, count: u32, class: PairClass) -> FilePair {
    let (a_p, b_p) = if a < b { (a, b) } else { (b, a) };
    FilePair {
        a: PathBuf::from(a_p),
        b: PathBuf::from(b_p),
        count,
        direction: None,
        class: Some(class),
    }
}

fn report(pairs: Vec<FilePair>) -> ChangeCouplingReport {
    ChangeCouplingReport {
        pairs,
        ..Default::default()
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

fn observer_reports_with_pairs(pairs: Vec<FilePair>) -> ObserverReports {
    let cc = report(pairs);
    ObserverReports {
        loc: heal_cli::observer::code::loc::LocReport::default(),
        complexity: heal_cli::observer::code::complexity::ComplexityReport::default(),
        complexity_observer: heal_cli::observer::code::complexity::ComplexityObserver::default(),
        churn: None,
        change_coupling: Some(cc),
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
        test_hotspot: None,
        doc_hotspot: None,
    }
}

fn cfg_test_enabled() -> Config {
    let mut cfg = Config::default();
    cfg.features.test = TestConfig {
        enabled: true,
        ..TestConfig::default()
    };
    cfg
}

#[test]
fn test_pair_below_p50_retags_as_drift_when_feature_on() {
    // Joint count = 3, project's p50 pair count = 8 → drift.
    let pairs = vec![classified_pair(
        "src/foo.test.ts",
        "src/foo.ts",
        3,
        PairClass::TestSrc,
    )];
    let reports = observer_reports_with_pairs(pairs);
    let cfg = cfg_test_enabled();
    let cal = calibration_with_p50(8.0);

    let registry = FeatureRegistry::builtin();
    let findings = registry.lower_all(&reports, &cfg, &cal);
    let drift_count = findings
        .iter()
        .filter(|f| f.metric == Finding::METRIC_CHANGE_COUPLING_DRIFT)
        .count();
    assert_eq!(
        drift_count, 1,
        "expected one drift finding, got {findings:#?}"
    );
}

#[test]
fn test_pair_above_p50_stays_expected_when_feature_on() {
    // Joint count = 12, p50 = 8 → healthy coupling, stays as `expected`.
    let pairs = vec![classified_pair(
        "src/foo.test.ts",
        "src/foo.ts",
        12,
        PairClass::TestSrc,
    )];
    let reports = observer_reports_with_pairs(pairs);
    let cfg = cfg_test_enabled();
    let cal = calibration_with_p50(8.0);

    let registry = FeatureRegistry::builtin();
    let findings = registry.lower_all(&reports, &cfg, &cal);
    assert!(findings
        .iter()
        .all(|f| f.metric != Finding::METRIC_CHANGE_COUPLING_DRIFT));
    assert!(findings
        .iter()
        .any(|f| f.metric == Finding::METRIC_CHANGE_COUPLING_EXPECTED));
}

#[test]
fn test_pair_drift_is_disabled_when_feature_off() {
    // Same low count as the first test, but `[features.test]` off →
    // pair stays demoted to `expected`.
    let pairs = vec![classified_pair(
        "src/foo.test.ts",
        "src/foo.ts",
        3,
        PairClass::TestSrc,
    )];
    let reports = observer_reports_with_pairs(pairs);
    let cfg = Config::default();
    let cal = calibration_with_p50(8.0);

    let registry = FeatureRegistry::builtin();
    let findings = registry.lower_all(&reports, &cfg, &cal);
    assert!(findings
        .iter()
        .all(|f| f.metric != Finding::METRIC_CHANGE_COUPLING_DRIFT));
}

#[test]
fn doc_pair_does_not_drift_even_when_below_p50() {
    // DocSrc pairs with low counts are demoted to expected, but never
    // promoted to drift — drift is a test-quality signal.
    let pairs = vec![classified_pair(
        "docs/cli.md",
        "src/cli.rs",
        3,
        PairClass::DocSrc,
    )];
    let reports = observer_reports_with_pairs(pairs);
    let cfg = cfg_test_enabled();
    let cal = calibration_with_p50(8.0);

    let registry = FeatureRegistry::builtin();
    let findings = registry.lower_all(&reports, &cfg, &cal);
    let docs_drift = findings
        .iter()
        .filter(|f| f.metric == Finding::METRIC_CHANGE_COUPLING_DRIFT)
        .count();
    assert_eq!(docs_drift, 0);
}

#[test]
fn drift_severity_is_medium_not_advisory() {
    let pairs = vec![classified_pair(
        "src/foo.test.ts",
        "src/foo.ts",
        3,
        PairClass::TestSrc,
    )];
    let reports = observer_reports_with_pairs(pairs);
    let cfg = cfg_test_enabled();
    let cal = calibration_with_p50(8.0);

    let registry = FeatureRegistry::builtin();
    let findings = registry.lower_all(&reports, &cfg, &cal);
    let drift = findings
        .iter()
        .find(|f| f.metric == Finding::METRIC_CHANGE_COUPLING_DRIFT)
        .expect("drift finding present");
    assert_eq!(drift.severity, Severity::Medium);
}
