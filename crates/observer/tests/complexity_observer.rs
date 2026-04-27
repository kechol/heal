//! Integration coverage for `ComplexityObserver`: tempdir-based fixtures
//! exercising the observer's project-walk + per-file analysis pipeline.

use heal_core::config::Config;
#[cfg(feature = "lang-rust")]
use heal_observer::complexity::ComplexityMetric;
use heal_observer::complexity::ComplexityObserver;

mod common;
#[allow(unused_imports)]
use common::write;

#[allow(dead_code)]
fn enabled_observer() -> ComplexityObserver {
    ComplexityObserver {
        excluded: Vec::new(),
        ccn_enabled: true,
        cognitive_enabled: true,
    }
}

#[test]
fn returns_empty_report_when_both_metrics_disabled() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/lib.rs", "fn one() { if true {} }\n");

    let observer = ComplexityObserver {
        excluded: Vec::new(),
        ccn_enabled: false,
        cognitive_enabled: false,
    };
    let report = observer.scan(dir.path());
    assert!(report.files.is_empty());
    assert_eq!(report.totals.functions, 0);
}

#[cfg(all(feature = "lang-ts", feature = "lang-rust"))]
#[test]
fn aggregates_metrics_across_typescript_and_rust() {
    let dir = tempfile::tempdir().unwrap();
    // Outer.rs: one fn with one if → CCN 2, Cognitive 1.
    write(
        dir.path(),
        "src/outer.rs",
        "fn outer(a: bool) -> i32 { if a { 1 } else { 0 } }\n",
    );
    // App.ts: one fn with two ifs (sequential) → CCN 3, Cognitive 2.
    write(
        dir.path(),
        "src/app.ts",
        "function app(a: number, b: number) { if (a > 0) {} if (b > 0) {} }\n",
    );

    let report = enabled_observer().scan(dir.path());
    assert_eq!(report.files.len(), 2, "got files: {:?}", report.files);

    // Files are sorted by path; src/app.ts < src/outer.rs.
    let app = &report.files[0];
    assert_eq!(app.language, "typescript");
    assert_eq!(app.functions.len(), 1);
    assert_eq!(app.functions[0].ccn, 3);

    let outer = &report.files[1];
    assert_eq!(outer.language, "rust");
    assert_eq!(outer.functions.len(), 1);
    assert_eq!(outer.functions[0].ccn, 2);
    assert_eq!(outer.functions[0].cognitive, 2); // if + else block

    assert_eq!(report.totals.files, 2);
    assert_eq!(report.totals.functions, 2);
    assert_eq!(report.totals.max_ccn, 3);
}

#[cfg(feature = "lang-rust")]
#[test]
fn excluded_substrings_skip_files() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/keep.rs", "fn k() {}\n");
    write(dir.path(), "vendor/skip.rs", "fn s() { if true {} }\n");

    let observer = ComplexityObserver {
        excluded: vec!["vendor".to_string()],
        ccn_enabled: true,
        cognitive_enabled: true,
    };
    let report = observer.scan(dir.path());
    assert_eq!(report.files.len(), 1);
    assert!(report.files[0].path.ends_with("keep.rs"));
}

#[cfg(feature = "lang-rust")]
#[test]
fn gitignore_is_respected() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".gitignore", "target/\n");
    write(dir.path(), "src/lib.rs", "fn a() {}\n");
    write(
        dir.path(),
        "target/debug.rs",
        "fn ignored() { if true {} }\n",
    );

    let report = enabled_observer().scan(dir.path());
    let paths: Vec<String> = report
        .files
        .iter()
        .map(|f| f.path.to_string_lossy().into_owned())
        .collect();
    assert!(
        paths.iter().any(|p| p.ends_with("lib.rs")),
        "expected lib.rs in {paths:?}"
    );
    assert!(
        paths.iter().all(|p| !p.contains("target")),
        "target/ should be gitignored, got {paths:?}"
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn worst_n_orders_descending_by_metric() {
    let dir = tempfile::tempdir().unwrap();
    // Three Rust functions with distinct CCN.
    write(
        dir.path(),
        "src/lo.rs",
        "fn lo() { if true {} }\n", // CCN 2
    );
    write(
        dir.path(),
        "src/mid.rs",
        "fn mid() { if true {} if true {} }\n", // CCN 3
    );
    write(
        dir.path(),
        "src/hi.rs",
        "fn hi() { if true {} if true {} if true {} }\n", // CCN 4
    );

    let report = enabled_observer().scan(dir.path());
    let top2 = report.worst_n(2, ComplexityMetric::Ccn);
    assert_eq!(top2.len(), 2);
    assert_eq!(top2[0].name, "hi", "got {top2:?}");
    assert_eq!(top2[0].ccn, 4);
    assert_eq!(top2[1].name, "mid");
    assert_eq!(top2[1].ccn, 3);

    let top1_cog = report.worst_n(1, ComplexityMetric::Cognitive);
    assert_eq!(top1_cog.len(), 1);
    // hi.rs has 3 sequential ifs at depth 0: 1+1+1 = 3 cognitive.
    assert_eq!(top1_cog[0].name, "hi");
    assert_eq!(top1_cog[0].cognitive, 3);
}

#[test]
fn from_config_inherits_excludes_and_toggles() {
    let mut cfg = Config::default();
    cfg.git.exclude_paths = vec!["dist".to_string()];
    cfg.metrics.loc.exclude_paths = vec!["vendor".to_string()];
    // ccn + cognitive are enabled by default in MetricsConfig::default().

    let observer = ComplexityObserver::from_config(&cfg);
    assert_eq!(
        observer.excluded,
        vec!["dist".to_string(), "vendor".to_string()]
    );
    assert!(observer.ccn_enabled);
    assert!(observer.cognitive_enabled);
}
