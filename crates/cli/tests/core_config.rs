use heal_cli::core::config::{
    assign_workspace, Config, CrossWorkspacePolicy, DrainSpec, DrainTier, HotspotMatch,
};
use heal_cli::core::finding::{Finding, Location};
use heal_cli::core::severity::Severity;
use std::path::{Path, PathBuf};

#[test]
fn empty_toml_yields_recommended_metric_defaults() {
    let cfg: Config = Config::from_toml_str("").unwrap();
    // `metrics.disabled` is empty by default — every code metric is on.
    assert!(cfg.metrics.disabled.is_empty());
    assert!(cfg.metrics.is_enabled("churn"));
    assert!(cfg.metrics.is_enabled("hotspot"));
    assert!(cfg.metrics.is_enabled("duplication"));
    assert!(cfg.metrics.is_enabled("ccn"));
    assert!(cfg.metrics.is_enabled("cognitive"));
    assert!(cfg.metrics.is_enabled("change_coupling"));
    assert!(cfg.metrics.is_enabled("lcom"));
    assert!(cfg.metrics.loc.inherit_git_excludes);
    assert!(cfg.metrics.loc.exclude_paths.is_empty());
    assert_eq!(cfg.metrics.top_n, 5);
    assert_eq!(cfg.git.since_days, 90);
}

#[test]
fn metrics_disabled_list_round_trips() {
    let cfg = r#"
        [metrics]
        disabled = ["lcom", "duplication"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    assert_eq!(parsed.metrics.disabled, vec!["lcom", "duplication"]);
    assert!(!parsed.metrics.is_enabled("lcom"));
    assert!(!parsed.metrics.is_enabled("duplication"));
    // Other metrics stay enabled.
    assert!(parsed.metrics.is_enabled("ccn"));
    assert!(parsed.metrics.is_enabled("cognitive"));
}

#[test]
fn metrics_disabled_rejects_unknown_metric() {
    let cfg = r#"
        [metrics]
        disabled = ["bogus"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let err = parsed
        .validate(std::path::Path::new("/tmp/cfg.toml"))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("bogus"),
        "validator must surface the offending name, got: {err}",
    );
}

#[test]
fn metrics_disabled_rejects_loc() {
    let cfg = r#"
        [metrics]
        disabled = ["loc"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let err = parsed
        .validate(std::path::Path::new("/tmp/cfg.toml"))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("loc"),
        "validator must reject `loc` explicitly, got: {err}",
    );
}

#[test]
fn legacy_per_metric_enabled_field_is_rejected() {
    // Pre-rename configs with `[metrics.<m>] enabled = true/false`
    // surface as `deny_unknown_fields` schema errors so the user
    // gets a clear migration nudge.
    let cfg = r"
        [metrics.lcom]
        enabled = false
    ";
    let err = Config::from_toml_str(cfg).unwrap_err().to_string();
    assert!(
        err.contains("enabled") || err.contains("unknown"),
        "expected schema error pointing at the legacy key, got: {err}",
    );
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
        top_n = 20

        [metrics.hotspot]
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
        top_n = 5
        unknown_key = "oops"
    "#;
    let err = Config::from_toml_str(bad).unwrap_err().to_string();
    assert!(err.contains("unknown_key"), "got: {err}");
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
        language = "typescript"
        exclude_paths = ["dist/**"]

        [[project.workspaces]]
        path = "services/api"
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    assert_eq!(parsed.project.workspaces.len(), 2);
    assert_eq!(parsed.project.workspaces[0].path, "packages/web");
    assert_eq!(
        parsed.project.workspaces[0].language.as_deref(),
        Some("typescript")
    );
    assert_eq!(
        parsed.project.workspaces[0].exclude_paths,
        vec!["dist/**".to_string()]
    );
    assert_eq!(parsed.project.workspaces[1].path, "services/api");
    assert!(parsed.project.workspaces[1].language.is_none());
}

#[test]
fn workspaces_legacy_primary_language_key_is_rejected() {
    // The pre-rename key surfaces as a `deny_unknown_fields` error so
    // the user gets a clear migration nudge — silent acceptance would
    // mask configs that still rely on the old name.
    let cfg = r#"
        [[project.workspaces]]
        path = "packages/web"
        primary_language = "typescript"
    "#;
    let err = Config::from_toml_str(cfg).unwrap_err().to_string();
    assert!(
        err.contains("primary_language") || err.contains("unknown"),
        "expected schema error pointing at the legacy key, got: {err}",
    );
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
fn hotspot_floor_ok_override_round_trips() {
    let cfg = r"
        [metrics.hotspot]
        floor_ok = 50.0
    ";
    let parsed = Config::from_toml_str(cfg).unwrap();
    assert_eq!(parsed.metrics.hotspot.floor_ok, Some(50.0));
}

#[test]
fn workspace_metric_overrides_round_trip() {
    let cfg = r#"
        [[project.workspaces]]
        path = "packages/web"

        [project.workspaces.metrics.ccn]
        floor_critical = 40
        floor_ok = 14

        [project.workspaces.metrics.duplication]
        floor_critical = 50
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let ws = &parsed.project.workspaces[0];
    assert_eq!(ws.metrics.ccn.floor_critical, Some(40.0));
    assert_eq!(ws.metrics.ccn.floor_ok, Some(14.0));
    assert_eq!(ws.metrics.duplication.floor_critical, Some(50.0));
    // Untouched metrics stay at defaults.
    assert!(ws.metrics.cognitive.is_empty());
    assert!(ws.metrics.lcom.is_empty());
}

#[test]
fn workspace_metric_overrides_reject_unknown_field() {
    let cfg = r#"
        [[project.workspaces]]
        path = "packages/web"

        [project.workspaces.metrics.ccn]
        bogus = 1
    "#;
    assert!(Config::from_toml_str(cfg).is_err());
}

#[test]
fn workspace_exclude_paths_validate_accepts_empty_and_comment_lines() {
    // Gitignore lets empty lines and `#` comments pass through; both
    // are no-ops at match time but keep config blocks readable.
    let cfg = r##"
        [[project.workspaces]]
        path = "packages/web"
        exclude_paths = ["", "# hand-curated below", "vendor/"]
    "##;
    let parsed = Config::from_toml_str(cfg).unwrap();
    parsed
        .validate(Path::new("/heal/config.toml"))
        .expect("empty + comment lines should validate");
}

#[test]
fn workspace_exclude_paths_validate_accepts_anchored_pattern() {
    // Leading `/` is gitignore-significant (anchor to workspace
    // root) — the translator preserves it as `/<workspace>/<rest>`.
    let cfg = r#"
        [[project.workspaces]]
        path = "packages/web"
        exclude_paths = ["/build", "/dist/"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    parsed
        .validate(Path::new("/heal/config.toml"))
        .expect("anchored workspace excludes are valid");
}

#[test]
fn workspace_exclude_paths_validate_rejects_dotdot() {
    let cfg = r#"
        [[project.workspaces]]
        path = "packages/web"
        exclude_paths = ["../escape"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    assert!(parsed.validate(Path::new("/heal/config.toml")).is_err());
}

#[test]
fn exclude_lines_translates_unanchored_workspace_excludes() {
    // `vendor/` is unanchored — under workspace `packages/web` it
    // becomes `packages/web/**/vendor/` so a gitignore matcher fires
    // on `packages/web/foo/vendor/x.ts`. `packages/api/vendor/...`
    // is unaffected (the prefix doesn't match).
    let cfg = r#"
        [git]
        exclude_paths = ["target/"]

        [[project.workspaces]]
        path = "packages/web"
        exclude_paths = ["vendor/", "generated/"]

        [[project.workspaces]]
        path = "packages/api"
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let lines = parsed.exclude_lines();
    assert!(lines.iter().any(|p| p == "target/"));
    assert!(lines.iter().any(|p| p == "packages/web/**/vendor/"));
    assert!(lines.iter().any(|p| p == "packages/web/**/generated/"));
    // packages/api declared no exclude_paths.
    assert!(!lines.iter().any(|p| p.starts_with("packages/api/")));
}

#[test]
fn exclude_lines_translates_anchored_workspace_excludes() {
    // Leading `/` under a workspace anchors at workspace root; the
    // translation re-anchors at project root via `/<ws>/<rest>`.
    let cfg = r#"
        [[project.workspaces]]
        path = "pkg/web"
        exclude_paths = ["/build", "/dist/"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let lines = parsed.exclude_lines();
    assert!(lines.iter().any(|p| p == "/pkg/web/build"));
    assert!(lines.iter().any(|p| p == "/pkg/web/dist/"));
}

#[test]
fn exclude_lines_preserves_workspace_negation() {
    // `!keep.log` should re-attach the `!` after body translation.
    let cfg = r#"
        [[project.workspaces]]
        path = "pkg/web"
        exclude_paths = ["*.log", "!keep.log"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    let lines = parsed.exclude_lines();
    assert!(lines.iter().any(|p| p == "pkg/web/**/*.log"));
    assert!(lines.iter().any(|p| p == "!pkg/web/**/keep.log"));
}

#[test]
fn validate_accepts_well_formed_gitignore_patterns() {
    // Sanity: every common gitignore feature parses through the
    // validation step. Glob, anchored, directory-only, negation,
    // workspace-translated patterns all build cleanly. The
    // `r##".."##` delimiter lets us embed `"#` (a literal hash inside
    // the gitignore comment line) without prematurely closing the
    // raw string.
    let cfg = r##"
        [git]
        exclude_paths = [
            "target/",
            "*.log",
            "/build",
            "**/__snapshots__/",
            "!keep.log",
            "# comment line",
        ]

        [[project.workspaces]]
        path = "pkg/web"
        exclude_paths = ["vendor/", "**/*.tmp", "/dist", "!keep.tmp"]
    "##;
    let parsed = Config::from_toml_str(cfg).unwrap();
    parsed
        .validate(Path::new("/heal/config.toml"))
        .expect("well-formed gitignore patterns should validate");
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
fn features_docs_default_disabled() {
    let cfg: Config = Config::from_toml_str("").unwrap();
    assert!(!cfg.features.docs.enabled);
    assert_eq!(cfg.features.docs.pairs_path, ".heal/doc_pairs.json");
    assert_eq!(cfg.features.docs.doc_freshness.high_commits, 5);
    assert_eq!(cfg.features.docs.doc_freshness.critical_commits, 20);
    // Standalone defaults pre-populate sensible Markdown / RST globs.
    assert!(cfg
        .features
        .docs
        .standalone
        .include
        .iter()
        .any(|p| p == "**/*.md"));
    // ADR / CHANGELOG live outside the drift universe by default.
    assert!(cfg
        .features
        .docs
        .standalone
        .exclude
        .iter()
        .any(|p| p.starts_with("CHANGELOG")));
}

#[test]
fn features_docs_enable_round_trips() {
    let cfg = r#"
        [features.docs]
        enabled = true
        pairs_path = ".heal/custom_pairs.json"

        [features.docs.doc_freshness]
        high_commits = 3
        critical_commits = 12

        [features.docs.standalone]
        include = ["docs/**/*.md"]
        exclude = ["docs/legacy/**"]
    "#;
    let parsed = Config::from_toml_str(cfg).unwrap();
    assert!(parsed.features.docs.enabled);
    assert_eq!(parsed.features.docs.pairs_path, ".heal/custom_pairs.json");
    assert_eq!(parsed.features.docs.doc_freshness.high_commits, 3);
    assert_eq!(parsed.features.docs.doc_freshness.critical_commits, 12);
    assert_eq!(
        parsed.features.docs.standalone.include,
        vec!["docs/**/*.md".to_string()]
    );
    assert_eq!(
        parsed.features.docs.standalone.exclude,
        vec!["docs/legacy/**".to_string()]
    );
}

#[test]
fn features_docs_rejects_unknown_field() {
    let bad = r"
        [features.docs]
        enabled = true
        bogus = 1
    ";
    assert!(Config::from_toml_str(bad).is_err());
}

#[test]
fn features_docs_freshness_rejects_unknown_field() {
    let bad = r"
        [features.docs.doc_freshness]
        bogus = 1
    ";
    assert!(Config::from_toml_str(bad).is_err());
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
