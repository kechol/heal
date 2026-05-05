//! `heal metrics` — render observer outputs as a human summary or
//! `--json` payload.
//!
//! Every invocation runs the observers fresh and renders directly —
//! no `.heal/snapshots/` reads, no historical delta. The orchestrator
//! dispatches over the section registry; no metric-specific branching
//! here.

mod code;
mod docs;
mod section;
mod test;

use std::io::{self, Write};
use std::path::Path;

use anyhow::Result;
use serde_json::json;

use crate::cli::MetricKind;
use crate::core::config::load_from_project;
use crate::core::term::write_through_pager;
use crate::core::HealPaths;
use crate::feature::Family;
use crate::observers::run_all;

use section::{all_sections, SectionCtx};

pub fn run(
    project: &Path,
    json_output: bool,
    metric: Option<MetricKind>,
    feature: Option<Family>,
    workspace: Option<&Path>,
    no_pager: bool,
) -> Result<()> {
    let paths = HealPaths::new(project);
    let cfg_exists = paths.config().exists();

    let cfg = if cfg_exists {
        Some(load_from_project(project)?)
    } else {
        None
    };

    // Early-exit when `--feature <disabled>` would otherwise produce
    // an empty payload. Mirrors `heal status`'s contract — skills
    // shell out and read the exit code to decide whether to bail.
    if let (Some(family), Some(cfg_ref)) = (feature, cfg.as_ref()) {
        if !family.is_enabled(cfg_ref) {
            eprintln!(
                "heal metrics: --feature {0} requested but `[features.{0}].enabled = false`. \
                 Edit `.heal/config.toml` (or run `/heal-setup`) to enable the family before re-running.",
                family.name(),
            );
            std::process::exit(1);
        }
    }

    let reports = cfg.as_ref().map(|c| run_all(project, c, metric, workspace));

    let sections = all_sections();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&build_json(
                cfg_exists,
                cfg.as_ref(),
                reports.as_ref(),
                metric,
                feature,
                workspace,
                &sections,
            ))?,
        );
        return Ok(());
    }

    if !cfg_exists {
        println!("HEAL is not initialized in this project. Run `heal init` first.");
        return Ok(());
    }
    let cfg = cfg.expect("cfg_exists branch implies cfg loaded");
    let reports = reports.expect("cfg present implies reports built");
    write_through_pager(no_pager, |w, colorize| {
        let ctx = SectionCtx {
            cfg: &cfg,
            reports: &reports,
            colorize,
        };
        write_header(w, project, &paths, workspace)?;
        for s in &sections {
            if !matches_metric(metric, s.metric()) {
                continue;
            }
            if !matches_family(feature, s.metric()) {
                continue;
            }
            s.render_text(&ctx, w)?;
        }
        Ok(())
    })
}

fn write_header(
    w: &mut dyn Write,
    project: &Path,
    paths: &HealPaths,
    workspace: Option<&Path>,
) -> io::Result<()> {
    writeln!(w, "HEAL metrics (project: {})", project.display())?;
    writeln!(w, "  config:            {}", paths.config().display())?;
    if let Some(ws) = workspace {
        writeln!(w, "  workspace:         {}", ws.display())?;
    }
    Ok(())
}

/// `None` means "no filter, print everything"; otherwise print only when
/// the section matches the requested metric.
fn matches_metric(filter: Option<MetricKind>, section: MetricKind) -> bool {
    filter.is_none_or(|f| f == section)
}

/// `None` means "no family filter"; otherwise print only when the
/// section's metric belongs to the requested family. Resolves via
/// `Family::for_metric` so the section list and `Finding.family()`
/// stay in lock-step against one canonical map.
fn matches_family(filter: Option<Family>, section: MetricKind) -> bool {
    filter.is_none_or(|f| Family::for_metric(section.json_key()) == f)
}

fn build_json(
    cfg_exists: bool,
    cfg: Option<&crate::core::config::Config>,
    reports: Option<&crate::observers::ObserverReports>,
    metric: Option<MetricKind>,
    feature: Option<Family>,
    workspace: Option<&Path>,
    sections: &[Box<dyn section::MetricSection>],
) -> serde_json::Value {
    let mut payload = serde_json::Map::new();
    payload.insert("initialized".into(), json!(cfg_exists));
    if let Some(m) = metric {
        payload.insert("metric".into(), json!(m.json_key()));
    }
    if let Some(f) = feature {
        payload.insert(
            "feature".into(),
            json!(match f {
                Family::Code => "code",
                Family::Test => "test",
                Family::Docs => "docs",
            }),
        );
    }
    if let Some(ws) = workspace {
        payload.insert("workspace".into(), json!(ws.display().to_string()));
    }
    if let (Some(cfg), Some(reports)) = (cfg, reports) {
        let ctx = SectionCtx {
            cfg,
            reports,
            colorize: false,
        };
        // Raw reports balloon for large repos (the `worst` precomputation
        // already captures what filtered consumers need); only emit them
        // in the unfiltered path so `--metric X --json` stays lean for
        // skill consumption. `--feature X` keeps the raw shape but
        // narrows the included keys to that family's metrics.
        if metric.is_none() {
            for s in sections {
                if !matches_family(feature, s.metric()) {
                    continue;
                }
                payload.insert(s.metric().json_key().into(), s.raw_json(&ctx));
            }
        } else if let Some(m) = metric {
            if let Some(s) = sections.iter().find(|s| s.metric() == m) {
                let (top_n, worst) = s.worst_json(&ctx);
                payload.insert("top_n".into(), json!(top_n));
                payload.insert("worst".into(), worst);
            }
        }
    }
    serde_json::Value::Object(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;
    use crate::observers::run_all;
    use crate::test_support::{commit, init_repo};
    use tempfile::TempDir;

    fn init_project(dir: &Path) {
        init_repo(dir);
        commit(
            dir,
            "lib.rs",
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
            "solo@example.com",
            "init",
        );
        let paths = HealPaths::new(dir);
        paths.ensure().unwrap();
        Config::default().save(&paths.config()).unwrap();
    }

    #[test]
    fn matches_metric_passes_when_filter_none_or_equal() {
        assert!(matches_metric(None, MetricKind::Loc));
        assert!(matches_metric(Some(MetricKind::Loc), MetricKind::Loc));
        assert!(!matches_metric(Some(MetricKind::Loc), MetricKind::Churn));
    }

    #[test]
    fn matches_family_routes_via_family_for_metric() {
        assert!(matches_family(None, MetricKind::Loc));
        assert!(matches_family(Some(Family::Code), MetricKind::Loc));
        assert!(!matches_family(Some(Family::Test), MetricKind::Loc));
        assert!(matches_family(Some(Family::Test), MetricKind::CoveragePct));
        assert!(matches_family(Some(Family::Docs), MetricKind::DocFreshness));
    }

    #[test]
    fn run_text_uninitialized_returns_ok() {
        let dir = TempDir::new().unwrap();
        // No `heal init` — exercise the "HEAL is not initialized" branch.
        run(dir.path(), false, None, None, None, true).unwrap();
    }

    #[test]
    fn run_json_uninitialized_returns_initialized_false() {
        let dir = TempDir::new().unwrap();
        let cfg_exists = HealPaths::new(dir.path()).config().exists();
        assert!(!cfg_exists);
        let payload = build_json(cfg_exists, None, None, None, None, None, &all_sections());
        assert_eq!(payload["initialized"], serde_json::Value::Bool(false));
        // Without cfg/reports, neither metric data nor the `worst`
        // payload should be present — only the `initialized` flag.
        assert!(payload.get("loc").is_none());
        assert!(payload.get("worst").is_none());
    }

    #[test]
    fn run_text_initialized_walks_every_section_render() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        // Text mode end-to-end. `no_pager = true` so tests never spawn
        // `less`. Asserts that every section's `render_text` returns
        // `Ok(())` against a real (but minimal) project.
        run(dir.path(), false, None, None, None, true).unwrap();
    }

    #[test]
    fn run_json_initialized_unfiltered_returns_ok() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        run(dir.path(), true, None, None, None, true).unwrap();
    }

    #[test]
    fn run_json_with_metric_filter_returns_ok() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        run(dir.path(), true, Some(MetricKind::Loc), None, None, true).unwrap();
    }

    #[test]
    fn run_json_with_feature_filter_returns_ok() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        run(dir.path(), true, None, Some(Family::Code), None, true).unwrap();
    }

    #[test]
    fn build_json_unfiltered_includes_every_section_key() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        let cfg = load_from_project(dir.path()).unwrap();
        let reports = run_all(dir.path(), &cfg, None, None);
        let sections = all_sections();

        let payload = build_json(
            true,
            Some(&cfg),
            Some(&reports),
            None,
            None,
            None,
            &sections,
        );
        assert_eq!(payload["initialized"], serde_json::Value::Bool(true));
        // Every section's `json_key` must appear under the unfiltered
        // path, regardless of whether its observer produced signal.
        // Code-family observers always run; docs/test default to off
        // and serialize as `null` — both cases are valid keys.
        for s in &sections {
            let key = s.metric().json_key();
            assert!(
                payload.get(key).is_some(),
                "unfiltered payload must include `{key}`",
            );
        }
        // The Loc observer always emits a non-null report for any
        // walkable project, so `payload[\"loc\"]` is the canary.
        assert!(
            !payload["loc"].is_null(),
            "loc must serialize as a non-null report",
        );
    }

    #[test]
    fn build_json_metric_filter_emits_top_n_and_worst() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        let cfg = load_from_project(dir.path()).unwrap();
        let reports = run_all(dir.path(), &cfg, Some(MetricKind::Loc), None);
        let sections = all_sections();

        let payload = build_json(
            true,
            Some(&cfg),
            Some(&reports),
            Some(MetricKind::Loc),
            None,
            None,
            &sections,
        );
        assert_eq!(payload["metric"], serde_json::Value::String("loc".into()));
        assert!(
            payload["top_n"].is_number(),
            "metric-filter path must echo the configured top_n",
        );
        assert!(
            payload.get("worst").is_some(),
            "metric-filter path must emit the worst payload",
        );
        // Per-section raw keys are excluded under `--metric`; only
        // `top_n` + `worst` carry the slice the skill consumes.
        assert!(payload.get("loc").is_none());
        assert!(payload.get("complexity").is_none());
    }

    #[test]
    fn build_json_feature_filter_narrows_to_family_keys() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        let cfg = load_from_project(dir.path()).unwrap();
        let reports = run_all(dir.path(), &cfg, None, None);
        let sections = all_sections();

        let payload = build_json(
            true,
            Some(&cfg),
            Some(&reports),
            None,
            Some(Family::Code),
            None,
            &sections,
        );
        assert_eq!(payload["feature"], serde_json::Value::String("code".into()));
        // Code-family keys present.
        for k in [
            "loc",
            "complexity",
            "churn",
            "change_coupling",
            "duplication",
            "hotspot",
            "lcom",
        ] {
            assert!(payload.get(k).is_some(), "code-family key `{k}` missing");
        }
        // Docs / test family keys absent.
        for k in [
            "doc_freshness",
            "doc_drift",
            "doc_coverage",
            "doc_link_health",
            "orphan_pages",
            "todo_density",
            "doc_hotspot",
            "coverage_pct",
            "skip_ratio",
            "test_hotspot",
        ] {
            assert!(
                payload.get(k).is_none(),
                "non-code key `{k}` leaked under `--feature code`",
            );
        }
    }

    #[test]
    fn build_json_workspace_echoes_path() {
        let dir = TempDir::new().unwrap();
        init_project(dir.path());
        let ws = dir.path().join("crates/foo");
        let cfg = load_from_project(dir.path()).unwrap();
        let reports = run_all(dir.path(), &cfg, None, Some(&ws));
        let sections = all_sections();
        let payload = build_json(
            true,
            Some(&cfg),
            Some(&reports),
            None,
            None,
            Some(&ws),
            &sections,
        );
        assert_eq!(
            payload["workspace"],
            serde_json::Value::String(ws.display().to_string()),
        );
    }
}
