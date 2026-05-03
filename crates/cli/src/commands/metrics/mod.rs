//! `heal metrics` — render observer outputs as a human summary or
//! `--json` payload.
//!
//! Every invocation runs the observers fresh and renders directly —
//! no `.heal/snapshots/` reads, no historical delta. Each metric is a
//! [`section::MetricSection`] in its own file: [`loc`], [`complexity`],
//! [`churn`], [`coupling`], [`duplication`], [`hotspot`], [`lcom`].
//! The orchestrator below loads config, runs observers, and dispatches
//! over the section registry — no metric-specific branching here.

mod churn;
mod complexity;
mod coupling;
mod duplication;
mod hotspot;
mod lcom;
mod loc;
mod section;

use std::io::{self, Write};
use std::path::Path;

use anyhow::Result;
use serde_json::json;

use crate::cli::MetricKind;
use crate::core::config::load_from_project;
use crate::core::HealPaths;
use crate::observers::run_all;

use section::{all_sections, SectionCtx};

pub fn run(
    project: &Path,
    json_output: bool,
    metric: Option<MetricKind>,
    workspace: Option<&Path>,
) -> Result<()> {
    let paths = HealPaths::new(project);
    let cfg_exists = paths.config().exists();

    let cfg = if cfg_exists {
        Some(load_from_project(project)?)
    } else {
        None
    };
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
    let ctx = SectionCtx {
        cfg: &cfg,
        reports: &reports,
    };

    let stdout = std::io::stdout();
    let mut w = stdout.lock();
    write_header(&mut w, project, &paths, workspace)?;
    for s in &sections {
        if !matches_metric(metric, s.metric()) {
            continue;
        }
        s.render_text(&ctx, &mut w)?;
    }
    Ok(())
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

fn build_json(
    cfg_exists: bool,
    cfg: Option<&crate::core::config::Config>,
    reports: Option<&crate::observers::ObserverReports>,
    metric: Option<MetricKind>,
    workspace: Option<&Path>,
    sections: &[Box<dyn section::MetricSection>],
) -> serde_json::Value {
    let mut payload = serde_json::Map::new();
    payload.insert("initialized".into(), json!(cfg_exists));
    if let Some(m) = metric {
        payload.insert("metric".into(), json!(m.json_key()));
    }
    if let Some(ws) = workspace {
        payload.insert("workspace".into(), json!(ws.display().to_string()));
    }
    if let (Some(cfg), Some(reports)) = (cfg, reports) {
        let ctx = SectionCtx { cfg, reports };
        // Raw reports balloon for large repos (the `worst` precomputation
        // already captures what filtered consumers need); only emit them
        // in the unfiltered path so `--metric X --json` stays lean for
        // skill consumption.
        if metric.is_none() {
            for s in sections {
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
