use heal_core::config::{Config, PolicyAction, ProjectProfile};

#[test]
fn empty_toml_yields_recommended_metric_defaults() {
    let cfg: Config = Config::from_toml_str("").unwrap();
    assert!(cfg.metrics.churn.enabled);
    assert!(cfg.metrics.hotspot.enabled);
    assert!(cfg.metrics.duplication.enabled);
    assert!(cfg.metrics.ccn.enabled);
    assert!(cfg.metrics.cognitive.enabled);
    assert!(cfg.metrics.doc_coverage.enabled);
    assert!(cfg.metrics.doc_update_skew.enabled);
    assert!(!cfg.metrics.bus_factor.enabled);
    assert!(!cfg.metrics.line_coverage.enabled);
    assert!(cfg.metrics.loc.inherit_git_excludes);
    assert!(cfg.metrics.loc.exclude_paths.is_empty());
    assert_eq!(cfg.metrics.top_n, 5);
    assert_eq!(cfg.git.since_days, 90);
    assert_eq!(cfg.agent.provider, "claude-code");
}

#[test]
fn metrics_top_n_round_trips() {
    let cfg = r"
        [metrics]
        top_n = 12
    ";
    let parsed = Config::from_toml_str(cfg).unwrap();
    assert_eq!(parsed.metrics.top_n, 12);
}

#[test]
fn per_metric_top_n_overrides_global() {
    let cfg = r"
        [metrics]
        top_n = 5

        [metrics.churn]
        enabled = true
        top_n = 20

        [metrics.hotspot]
        enabled = true
        top_n = 8
    ";
    let parsed = Config::from_toml_str(cfg).unwrap();
    let m = &parsed.metrics;
    assert_eq!(m.top_n, 5);
    assert_eq!(m.top_n_churn(), 20);
    assert_eq!(m.top_n_hotspot(), 8);
    // Untouched metrics fall back to the global.
    assert_eq!(m.top_n_loc(), 5);
    assert_eq!(m.top_n_complexity(), 5);
    assert_eq!(m.top_n_duplication(), 5);
    assert_eq!(m.top_n_change_coupling(), 5);
}

#[test]
fn loc_config_round_trips_with_overrides() {
    let cfg = r#"
        [metrics.loc]
        inherit_git_excludes = false
        exclude_paths = ["dist", "vendor"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    assert!(!parsed.metrics.loc.inherit_git_excludes);
    assert_eq!(
        parsed.metrics.loc.exclude_paths,
        vec!["dist".to_string(), "vendor".to_string()]
    );
}

#[test]
fn loc_section_rejects_unknown_fields() {
    let bad = r#"
        [metrics.loc]
        unknown = "oops"
    "#;
    let err = Config::from_toml_str(bad).unwrap_err().to_string();
    assert!(err.contains("unknown"), "got: {err}");
}

#[test]
fn programmatic_default_matches_serde_default() {
    let from_toml = Config::from_toml_str("").unwrap();
    let from_default = Config::default();
    assert_eq!(from_toml, from_default);
}

#[test]
fn deny_unknown_fields_in_metrics() {
    let bad = r#"
        [metrics.churn]
        enabled = true
        unknown_key = "oops"
    "#;
    let err = Config::from_toml_str(bad).unwrap_err().to_string();
    assert!(err.contains("unknown_key"), "got: {err}");
}

#[test]
fn policy_action_is_kebab_case() {
    let cfg = r#"
        [policy.high_complexity_new_function]
        action = "report-only"
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let policy = &parsed.policy["high_complexity_new_function"];
    assert!(matches!(policy.action, PolicyAction::ReportOnly));
    assert_eq!(policy.cooldown_hours, 24);
}

#[test]
fn save_then_load_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let cfg = Config::recommended(ProjectProfile::Solo);
    cfg.save(&path).unwrap();
    let reloaded = Config::load(&path).unwrap();
    assert_eq!(cfg, reloaded);
}

#[test]
fn load_missing_returns_config_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("absent.toml");
    let err = Config::load(&path).unwrap_err();
    assert!(
        matches!(err, heal_core::Error::ConfigMissing(_)),
        "got: {err}"
    );
}
