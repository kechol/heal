use heal_cli::core::config::{Config, LocConfig, MetricsConfig};
use heal_cli::observer::loc::{LocObserver, LocReport};
use heal_cli::observer::Observer;

mod common;
use common::write;

fn ts_file() -> &'static str {
    "// hi\nexport const a = 1;\nexport const b = 2;\nexport const c = 3;\n"
}

fn js_file() -> &'static str {
    "module.exports = { x: 1 };\n"
}

fn md_file() -> &'static str {
    "# Heading\n\nSome prose with `code`.\n"
}

#[test]
fn primary_picks_highest_code_non_literate_language() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/app.ts", ts_file());
    write(dir.path(), "src/util.js", js_file());
    write(dir.path(), "README.md", md_file());

    let report = LocObserver::default().scan(dir.path());
    assert_eq!(report.primary.as_deref(), Some("TypeScript"));
    let names: Vec<&str> = report.languages.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"TypeScript"));
    assert!(names.contains(&"JavaScript"));
}

#[test]
fn empty_tree_yields_no_primary() {
    let dir = tempfile::tempdir().unwrap();
    let report = LocObserver::default().scan(dir.path());
    assert!(report.primary.is_none());
    assert!(report.languages.is_empty());
    assert_eq!(report.totals.code, 0);
}

#[test]
fn literate_only_tree_has_no_primary_but_lists_markdown() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "README.md", md_file());
    write(dir.path(), "docs/intro.md", md_file());

    let report = LocObserver::default().scan(dir.path());
    assert!(report.primary.is_none());
    let names: Vec<&str> = report.languages.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"Markdown"));
}

#[test]
fn excluded_paths_skip_files() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/app.ts", ts_file());
    write(dir.path(), "node_modules/leftpad/index.js", js_file());

    let observer = LocObserver {
        excluded: vec!["node_modules".to_string()],
        ..LocObserver::default()
    };
    let report = observer.scan(dir.path());
    let names: Vec<&str> = report.languages.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"TypeScript"));
    assert!(!names.contains(&"JavaScript"), "got {names:?}");
}

#[test]
fn exclude_languages_drops_entries() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/app.ts", ts_file());
    write(dir.path(), "src/util.js", js_file());

    let observer = LocObserver {
        exclude_languages: vec!["JavaScript".to_string()],
        ..LocObserver::default()
    };
    let report = observer.scan(dir.path());
    let names: Vec<&str> = report.languages.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"TypeScript"));
    assert!(!names.contains(&"JavaScript"));
}

#[test]
fn from_config_inherits_git_excludes_by_default() {
    let mut cfg = Config::default();
    cfg.git.exclude_paths = vec!["dist".to_string()];
    cfg.metrics.loc.exclude_paths = vec!["vendor".to_string()];

    let observer = LocObserver::from_config(&cfg);
    assert_eq!(
        observer.excluded,
        vec!["dist".to_string(), "vendor".to_string()]
    );
}

#[test]
fn from_config_can_opt_out_of_git_excludes() {
    let cfg = Config {
        git: heal_cli::core::config::GitConfig {
            since_days: 90,
            exclude_paths: vec!["dist".to_string()],
        },
        metrics: MetricsConfig {
            loc: LocConfig {
                inherit_git_excludes: false,
                exclude_paths: vec!["vendor".to_string()],
                top_n: None,
            },
            ..MetricsConfig::default()
        },
        ..Config::default()
    };

    let observer = LocObserver::from_config(&cfg);
    assert_eq!(observer.excluded, vec!["vendor".to_string()]);
}

#[test]
fn observer_trait_returns_loc_meta() {
    let observer = LocObserver::default();
    let meta = observer.meta();
    assert_eq!(meta.name, "loc");
    assert_eq!(meta.version, 1);
}

#[test]
fn observe_matches_scan() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/app.ts", ts_file());

    let observer = LocObserver::default();
    let direct = observer.scan(dir.path());
    let via_trait: LocReport = observer.observe(dir.path()).unwrap();
    assert_eq!(direct, via_trait);
}
