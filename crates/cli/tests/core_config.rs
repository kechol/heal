use heal_cli::core::config::{
    assign_workspace, Config, CrossWorkspacePolicy, DrainSpec, DrainTier, HotspotMatch,
    PolicyAction,
};
use heal_cli::core::finding::{Finding, Location};
use heal_cli::core::severity::Severity;
use std::path::{Path, PathBuf};

#[test]
fn empty_toml_yields_recommended_metric_defaults() {
    let cfg: Config = Config::from_toml_str("").unwrap();
    assert!(cfg.metrics.churn.enabled);
    assert!(cfg.metrics.hotspot.enabled);
    assert!(cfg.metrics.duplication.enabled);
    assert!(cfg.metrics.ccn.enabled);
    assert!(cfg.metrics.cognitive.enabled);
    assert!(cfg.metrics.loc.inherit_git_excludes);
    assert!(cfg.metrics.loc.exclude_paths.is_empty());
    assert_eq!(cfg.metrics.top_n, 5);
    assert_eq!(cfg.git.since_days, 90);
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
        [policy.rules.high_complexity_new_function]
        action = "report-only"
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let policy = &parsed.policy.rules["high_complexity_new_function"];
    assert!(matches!(policy.action, PolicyAction::ReportOnly));
}

#[test]
fn save_then_load_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let cfg = Config::default();
    cfg.save(&path).unwrap();
    let reloaded = Config::load(&path).unwrap();
    assert_eq!(cfg, reloaded);
}

#[test]
fn drain_spec_dsl_round_trip() {
    let cfg = r#"
        [policy.drain]
        must = ["critical:hotspot"]
        should = ["critical", "high:hotspot"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    assert_eq!(
        parsed.policy.drain.must,
        vec![DrainSpec {
            severity: Severity::Critical,
            hotspot: HotspotMatch::Required,
        }]
    );
    assert_eq!(
        parsed.policy.drain.should,
        vec![
            DrainSpec {
                severity: Severity::Critical,
                hotspot: HotspotMatch::Any,
            },
            DrainSpec {
                severity: Severity::High,
                hotspot: HotspotMatch::Required,
            },
        ]
    );
}

#[test]
fn drain_spec_defaults_when_omitted() {
    // Empty config produces the v0.3 default drain policy:
    // must = critical:hotspot, should = critical + high:hotspot.
    let cfg: Config = Config::from_toml_str("").unwrap();
    assert_eq!(cfg.policy.drain.must.len(), 1);
    assert_eq!(cfg.policy.drain.should.len(), 2);
    assert_eq!(cfg.policy.drain.must[0].hotspot, HotspotMatch::Required);
}

#[test]
fn drain_spec_rejects_unknown_severity() {
    let cfg = r#"
        [policy.drain]
        must = ["urgent:hotspot"]
    "#;
    let err = Config::from_toml_str(cfg).unwrap_err().to_string();
    assert!(err.contains("unknown severity"), "got: {err}");
}

#[test]
fn drain_spec_rejects_unknown_flag() {
    let cfg = r#"
        [policy.drain]
        must = ["critical:churned"]
    "#;
    let err = Config::from_toml_str(cfg).unwrap_err().to_string();
    assert!(err.contains("unknown flag"), "got: {err}");
}

fn finding_with(severity: Severity, hotspot: bool) -> Finding {
    let mut f = Finding::new(
        "ccn",
        Location {
            file: PathBuf::from("src/x.ts"),
            line: Some(1),
            symbol: Some("fn".into()),
        },
        "CCN=42 fn".into(),
        "ccn",
    );
    f.severity = severity;
    f.hotspot = hotspot;
    f
}

#[test]
fn tier_for_default_policy_buckets_findings() {
    let cfg: Config = Config::from_toml_str("").unwrap();
    let drain = &cfg.policy.drain;

    // Critical 🔥 → must (T0)
    assert_eq!(
        drain.tier_for(&finding_with(Severity::Critical, true)),
        Some(DrainTier::Must)
    );
    // Critical (no hotspot) → should (T1) via "critical" spec
    assert_eq!(
        drain.tier_for(&finding_with(Severity::Critical, false)),
        Some(DrainTier::Should)
    );
    // High 🔥 → should via "high:hotspot"
    assert_eq!(
        drain.tier_for(&finding_with(Severity::High, true)),
        Some(DrainTier::Should)
    );
    // High (no hotspot) → advisory
    assert_eq!(
        drain.tier_for(&finding_with(Severity::High, false)),
        Some(DrainTier::Advisory)
    );
    // Medium → advisory
    assert_eq!(
        drain.tier_for(&finding_with(Severity::Medium, false)),
        Some(DrainTier::Advisory)
    );
    // Ok → excluded entirely
    assert_eq!(drain.tier_for(&finding_with(Severity::Ok, false)), None);
    assert_eq!(drain.tier_for(&finding_with(Severity::Ok, true)), None);
}

#[test]
fn tier_for_per_metric_override_replaces_global() {
    // ccn gets a stricter must list; cognitive falls back to the global.
    let cfg = r#"
        [policy.drain]
        must = ["critical:hotspot"]
        should = ["critical"]

        [policy.drain.metrics.ccn]
        must = ["critical:hotspot", "high:hotspot"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let drain = &parsed.policy.drain;
    let mut ccn = finding_with(Severity::High, true);
    ccn.metric = "ccn".into();
    let mut cog = finding_with(Severity::High, true);
    cog.metric = "cognitive".into();
    // ccn override picks up High 🔥 as must.
    assert_eq!(drain.tier_for(&ccn), Some(DrainTier::Must));
    // cognitive falls back to global — High 🔥 isn't in must or should
    // there, so Advisory.
    assert_eq!(drain.tier_for(&cog), Some(DrainTier::Advisory));
}

#[test]
fn tier_for_per_metric_partial_override_inherits() {
    // override sets only `must`; `should` inherits from global.
    let cfg = r#"
        [policy.drain]
        must = ["critical:hotspot"]
        should = ["critical", "high:hotspot"]

        [policy.drain.metrics.ccn]
        must = ["critical"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let drain = &parsed.policy.drain;
    let mut ccn = finding_with(Severity::High, true);
    ccn.metric = "ccn".into();
    // High 🔥 hits the inherited global `should`.
    assert_eq!(drain.tier_for(&ccn), Some(DrainTier::Should));
}

#[test]
fn tier_for_sub_metric_inherits_from_parent_override() {
    // change_coupling.symmetric should pick up the change_coupling
    // override before falling back to global.
    let cfg = r#"
        [policy.drain]
        must = ["critical:hotspot"]
        should = ["critical"]

        [policy.drain.metrics.change_coupling]
        must = ["critical", "high"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let drain = &parsed.policy.drain;
    let mut sub = finding_with(Severity::High, false);
    sub.metric = "change_coupling.symmetric".into();
    // No `change_coupling.symmetric` override — falls back to parent
    // `change_coupling`, which lists "high" in must.
    assert_eq!(drain.tier_for(&sub), Some(DrainTier::Must));
}

#[test]
fn tier_for_must_takes_precedence_over_should() {
    // If a custom policy lists "critical" in both must and should, must
    // wins (the iteration order in PolicyDrainConfig is must-then-should).
    let cfg = r#"
        [policy.drain]
        must = ["critical"]
        should = ["critical"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let drain = &parsed.policy.drain;
    assert_eq!(
        drain.tier_for(&finding_with(Severity::Critical, false)),
        Some(DrainTier::Must)
    );
}

#[test]
fn cross_workspace_default_is_surface() {
    let cfg = Config::from_toml_str("").unwrap();
    assert_eq!(
        cfg.metrics.change_coupling.cross_workspace,
        CrossWorkspacePolicy::Surface,
    );
}

#[test]
fn cross_workspace_hide_round_trips() {
    let cfg = r#"
        [metrics.change_coupling]
        enabled = true
        cross_workspace = "hide"
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    assert_eq!(
        parsed.metrics.change_coupling.cross_workspace,
        CrossWorkspacePolicy::Hide,
    );
}

#[test]
fn tier_for_cross_workspace_metric_routes_to_advisory_by_default() {
    // A finding tagged change_coupling.cross_workspace at Critical
    // severity must land in Advisory under default policy — the metric
    // is a "for awareness" surface, not a drain target. The parent
    // change_coupling spec (which would normally route critical→Must
    // via the global must list) must NOT take effect for this sub-metric.
    let cfg = Config::from_toml_str("").unwrap();
    let drain = &cfg.policy.drain;
    let mut f = finding_with(Severity::Critical, true);
    f.metric = "change_coupling.cross_workspace".into();
    assert_eq!(drain.tier_for(&f), Some(DrainTier::Advisory));
}

#[test]
fn tier_for_cross_workspace_metric_respects_explicit_override() {
    // Once the user adds a per-metric override, the default Advisory
    // routing yields and the override is honored normally.
    let cfg = r#"
        [policy.drain.metrics."change_coupling.cross_workspace"]
        must = ["critical:hotspot"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let drain = &parsed.policy.drain;
    let mut f = finding_with(Severity::Critical, true);
    f.metric = "change_coupling.cross_workspace".into();
    assert_eq!(drain.tier_for(&f), Some(DrainTier::Must));
}

#[test]
fn drain_spec_rejects_extra_segments() {
    let cfg = r#"
        [policy.drain]
        must = ["critical:hotspot:extra"]
    "#;
    let err = Config::from_toml_str(cfg).unwrap_err().to_string();
    assert!(err.contains("too many"), "got: {err}");
}

#[test]
fn load_missing_returns_config_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("absent.toml");
    let err = Config::load(&path).unwrap_err();
    assert!(
        matches!(err, heal_cli::core::Error::ConfigMissing(_)),
        "got: {err}"
    );
}

#[test]
fn workspaces_default_is_empty() {
    let cfg: Config = Config::from_toml_str("").unwrap();
    assert!(cfg.project.workspaces.is_empty());
}

#[test]
fn workspaces_round_trip() {
    let cfg = r#"
        [[project.workspaces]]
        path = "packages/web"
        primary_language = "typescript"
        exclude_paths = ["dist/**"]

        [[project.workspaces]]
        path = "services/api"
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    assert_eq!(parsed.project.workspaces.len(), 2);
    assert_eq!(parsed.project.workspaces[0].path, "packages/web");
    assert_eq!(
        parsed.project.workspaces[0].primary_language.as_deref(),
        Some("typescript")
    );
    assert_eq!(
        parsed.project.workspaces[0].exclude_paths,
        vec!["dist/**".to_string()]
    );
    assert_eq!(parsed.project.workspaces[1].path, "services/api");
    assert!(parsed.project.workspaces[1].primary_language.is_none());
}

#[test]
fn workspaces_unknown_field_rejected() {
    let cfg = r#"
        [[project.workspaces]]
        path = "packages/web"
        bogus = "x"
    "#;
    assert!(Config::from_toml_str(cfg).is_err());
}

#[test]
fn workspaces_validate_rejects_empty_path() {
    let cfg = r#"
        [[project.workspaces]]
        path = ""
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let err = parsed.validate(Path::new("/heal/config.toml")).unwrap_err();
    assert!(
        matches!(err, heal_cli::core::Error::ConfigInvalid { .. }),
        "got: {err}"
    );
}

#[test]
fn workspaces_validate_rejects_absolute_path() {
    let cfg = r#"
        [[project.workspaces]]
        path = "/etc/heal"
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    assert!(parsed.validate(Path::new("/heal/config.toml")).is_err());
}

#[test]
fn workspaces_validate_rejects_dotdot() {
    let cfg = r#"
        [[project.workspaces]]
        path = "packages/../etc"
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    assert!(parsed.validate(Path::new("/heal/config.toml")).is_err());
}

#[test]
fn workspaces_validate_rejects_duplicates() {
    let cfg = r#"
        [[project.workspaces]]
        path = "packages/web"
        [[project.workspaces]]
        path = "packages/web"
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    assert!(parsed.validate(Path::new("/heal/config.toml")).is_err());
}

#[test]
fn workspaces_validate_rejects_nesting() {
    let cfg = r#"
        [[project.workspaces]]
        path = "packages"
        [[project.workspaces]]
        path = "packages/web"
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let err = parsed.validate(Path::new("/heal/config.toml")).unwrap_err();
    assert!(
        format!("{err}").contains("nest"),
        "expected nesting error, got: {err}"
    );
}

#[test]
fn workspaces_validate_allows_sibling_prefixes() {
    // `pkg/web` is NOT a prefix of `pkg/webapp` (segment-wise), so
    // they coexist fine.
    let cfg = r#"
        [[project.workspaces]]
        path = "pkg/web"
        [[project.workspaces]]
        path = "pkg/webapp"
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    parsed
        .validate(Path::new("/heal/config.toml"))
        .expect("sibling prefixes are valid");
}

#[test]
fn assign_workspace_returns_none_when_no_workspaces_declared() {
    let result = assign_workspace(Path::new("packages/web/foo.ts"), &[]);
    assert!(result.is_none());
}

#[test]
fn assign_workspace_picks_matching_workspace() {
    let cfg = r#"
        [[project.workspaces]]
        path = "packages/web"
        [[project.workspaces]]
        path = "services/api"
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    parsed.validate(Path::new("/heal/config.toml")).unwrap();
    assert_eq!(
        assign_workspace(
            Path::new("packages/web/src/foo.ts"),
            &parsed.project.workspaces
        ),
        Some("packages/web")
    );
    assert_eq!(
        assign_workspace(Path::new("services/api/x.py"), &parsed.project.workspaces),
        Some("services/api")
    );
}

#[test]
fn assign_workspace_returns_none_for_files_outside_any_workspace() {
    let cfg = r#"
        [[project.workspaces]]
        path = "packages/web"
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    parsed.validate(Path::new("/heal/config.toml")).unwrap();
    assert_eq!(
        assign_workspace(Path::new("README.md"), &parsed.project.workspaces),
        None,
    );
    assert_eq!(
        assign_workspace(Path::new("scripts/build.sh"), &parsed.project.workspaces),
        None,
    );
}

#[test]
fn assign_workspace_segment_wise_match_not_substring() {
    let cfg = r#"
        [[project.workspaces]]
        path = "pkg/web"
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    parsed.validate(Path::new("/heal/config.toml")).unwrap();
    // `pkg/webapp/...` must NOT match `pkg/web` workspace (segment-wise).
    assert_eq!(
        assign_workspace(Path::new("pkg/webapp/index.ts"), &parsed.project.workspaces),
        None,
    );
}
