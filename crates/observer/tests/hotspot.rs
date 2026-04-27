//! Coverage for `hotspot::compose` (pure score computation) and the
//! end-to-end `HotspotObserver::scan` pipeline.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use heal_observer::churn::{ChurnObserver, ChurnReport, ChurnTotals, FileChurn};
use heal_observer::complexity::{
    ComplexityObserver, ComplexityReport, ComplexityTotals, FileComplexity, FunctionMetric,
};
use heal_observer::hotspot::{compose, HotspotObserver, HotspotWeights};

mod common;
#[allow(unused_imports)]
use common::{commit_files, init_repo, write};

#[allow(dead_code)]
fn now_secs() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap()
}

fn churn_report(items: &[(&str, u32)]) -> ChurnReport {
    let files: Vec<FileChurn> = items
        .iter()
        .map(|(path, commits)| FileChurn {
            path: PathBuf::from(path),
            commits: *commits,
            lines_added: 0,
            lines_deleted: 0,
        })
        .collect();
    let commits_total = items.iter().map(|(_, c)| *c).sum();
    ChurnReport {
        files,
        totals: ChurnTotals {
            files: items.len(),
            commits: commits_total,
            lines_added: 0,
            lines_deleted: 0,
        },
        since_days: 90,
    }
}

fn complexity_report(items: &[(&str, &[u32])]) -> ComplexityReport {
    let files: Vec<FileComplexity> = items
        .iter()
        .map(|(path, ccns)| FileComplexity {
            path: PathBuf::from(path),
            language: "rust".to_string(),
            functions: ccns
                .iter()
                .enumerate()
                .map(|(i, ccn)| FunctionMetric {
                    name: format!("f{i}"),
                    start_line: 1,
                    end_line: 1,
                    ccn: *ccn,
                    cognitive: 0,
                })
                .collect(),
        })
        .collect();
    let total_functions = items.iter().map(|(_, c)| c.len()).sum();
    let max_ccn = items
        .iter()
        .flat_map(|(_, c)| c.iter().copied())
        .max()
        .unwrap_or(0);
    ComplexityReport {
        files,
        totals: ComplexityTotals {
            files: items.len(),
            functions: total_functions,
            max_ccn,
            max_cognitive: 0,
        },
    }
}

#[test]
fn compose_multiplies_churn_and_ccn_sum() {
    let churn = churn_report(&[("src/a.rs", 10), ("src/b.rs", 2)]);
    let complexity = complexity_report(&[("src/a.rs", &[5, 5]), ("src/b.rs", &[20])]);
    // a: commits=10, ccn_sum=10 → score 100
    // b: commits=2,  ccn_sum=20 → score 40

    let report = compose(&churn, &complexity, HotspotWeights::default());
    assert_eq!(report.entries.len(), 2);
    assert_eq!(report.entries[0].path.to_string_lossy(), "src/a.rs");
    assert!((report.entries[0].score - 100.0).abs() < f64::EPSILON);
    assert_eq!(report.entries[1].path.to_string_lossy(), "src/b.rs");
    assert!((report.entries[1].score - 40.0).abs() < f64::EPSILON);
    assert!((report.totals.max_score - 100.0).abs() < f64::EPSILON);
}

#[test]
fn compose_applies_weights() {
    let churn = churn_report(&[("a.rs", 4)]);
    let complexity = complexity_report(&[("a.rs", &[5])]);

    let weights = HotspotWeights {
        churn: 2.0,
        complexity: 3.0,
    };
    let report = compose(&churn, &complexity, weights);
    // (2*4) * (3*5) = 8 * 15 = 120
    assert_eq!(report.entries.len(), 1);
    assert!((report.entries[0].score - 120.0).abs() < f64::EPSILON);
}

#[test]
fn compose_drops_files_missing_one_signal() {
    let churn = churn_report(&[("only_churn.rs", 5), ("both.rs", 3)]);
    let complexity = complexity_report(&[("both.rs", &[4]), ("only_complex.rs", &[10])]);
    let report = compose(&churn, &complexity, HotspotWeights::default());
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].path.to_string_lossy(), "both.rs");
}

#[test]
fn compose_skips_zero_ccn_or_zero_commits() {
    let churn = churn_report(&[("a.rs", 0), ("b.rs", 1)]);
    let complexity = complexity_report(&[("a.rs", &[5]), ("b.rs", &[])]);
    let report = compose(&churn, &complexity, HotspotWeights::default());
    assert!(report.entries.is_empty());
}

#[test]
fn empty_when_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let observer = HotspotObserver {
        enabled: false,
        weights: HotspotWeights::default(),
        churn: ChurnObserver::default(),
        complexity: ComplexityObserver::default(),
    };
    let report = observer.scan(dir.path());
    assert!(report.entries.is_empty());
}

#[cfg(feature = "lang-rust")]
#[test]
fn scan_runs_underlying_observers() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();

    write(
        dir.path(),
        "src/hot.rs",
        "fn h(a: bool, b: bool) -> i32 { if a { 1 } else if b { 2 } else { 3 } }\n",
    );
    write(
        dir.path(),
        "src/cold.rs",
        "fn c(a: bool) -> i32 { if a { 1 } else { 0 } }\n",
    );
    commit_files(
        &repo,
        &[
            (
                "src/hot.rs",
                "fn h(a: bool, b: bool) -> i32 { if a { 1 } else if b { 2 } else { 3 } }\n",
            ),
            (
                "src/cold.rs",
                "fn c(a: bool) -> i32 { if a { 1 } else { 0 } }\n",
            ),
        ],
        "init",
        now - 100,
    );
    // Bump hot.rs three more times to widen the churn gap.
    for i in 0..3 {
        commit_files(
            &repo,
            &[(
                "src/hot.rs",
                &format!("fn h(a: bool, b: bool) -> i32 {{ {} }}\n", i + 10),
            )],
            &format!("hot {i}"),
            now - 50 + i,
        );
    }

    let observer = HotspotObserver {
        enabled: true,
        weights: HotspotWeights::default(),
        churn: ChurnObserver {
            enabled: true,
            excluded: Vec::new(),
            since_days: 90,
        },
        complexity: ComplexityObserver {
            excluded: Vec::new(),
            ccn_enabled: true,
            cognitive_enabled: true,
        },
    };
    let report = observer.scan(dir.path());
    assert!(!report.entries.is_empty());
    // hot.rs should rank above cold.rs.
    let top = &report.entries[0];
    assert_eq!(top.path.to_string_lossy(), "src/hot.rs");
    assert!(top.churn_commits >= 4);
}
