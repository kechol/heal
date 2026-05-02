//! `heal metrics` — render observer outputs as a human summary or
//! `--json` payload.
//!
//! Each metric is a [`section::MetricSection`] in its own file:
//! [`loc`], [`complexity`], [`churn`], [`coupling`], [`duplication`],
//! [`hotspot`], [`lcom`]. The orchestrator below loads config, runs
//! observers, and dispatches over the section registry — no metric-
//! specific branching here.

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
use crate::core::eventlog::{Event, EventLog};
use crate::core::snapshot::{MetricsSnapshot, SnapshotDelta};
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
    let snapshot_segments = EventLog::new(paths.snapshots_dir()).segments()?;
    let segment_count = snapshot_segments.len();
    let snapshot_count = EventLog::iter_segments(snapshot_segments.clone())
        .flatten()
        .count();
    let latest = MetricsSnapshot::latest_in_segments(&snapshot_segments).unwrap_or(None);
    let delta = latest
        .as_ref()
        .and_then(|(_, m)| m.delta.as_ref())
        .and_then(|v| serde_json::from_value::<SnapshotDelta>(v.clone()).ok());

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
                segment_count,
                snapshot_count,
                cfg.as_ref(),
                reports.as_ref(),
                delta.as_ref(),
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
    write_header(
        &mut w,
        project,
        &paths,
        segment_count,
        snapshot_count,
        workspace,
    )?;
    for s in &sections {
        if !matches_metric(metric, s.metric()) {
            continue;
        }
        s.render_text(&ctx, &mut w)?;
    }
    if let (Some((snap, _)), Some(d)) = (latest.as_ref(), delta.as_ref()) {
        print_delta_summary(&mut w, snap, d, metric)?;
    }
    Ok(())
}

fn write_header(
    w: &mut dyn Write,
    project: &Path,
    paths: &HealPaths,
    segment_count: usize,
    snapshot_count: usize,
    workspace: Option<&Path>,
) -> io::Result<()> {
    writeln!(w, "HEAL metrics (project: {})", project.display())?;
    writeln!(w, "  config:            {}", paths.config().display())?;
    writeln!(w, "  snapshot segments: {segment_count}")?;
    writeln!(w, "  snapshots:         {snapshot_count}")?;
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

#[allow(clippy::too_many_arguments)]
fn build_json(
    cfg_exists: bool,
    segment_count: usize,
    snapshot_count: usize,
    cfg: Option<&crate::core::config::Config>,
    reports: Option<&crate::observers::ObserverReports>,
    delta: Option<&SnapshotDelta>,
    metric: Option<MetricKind>,
    workspace: Option<&Path>,
    sections: &[Box<dyn section::MetricSection>],
) -> serde_json::Value {
    let mut payload = serde_json::Map::new();
    payload.insert("initialized".into(), json!(cfg_exists));
    payload.insert("snapshot_segments".into(), json!(segment_count));
    payload.insert("snapshots".into(), json!(snapshot_count));
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
    payload.insert("delta".into(), filtered_delta(delta, metric));
    serde_json::Value::Object(payload)
}

/// Filter the delta payload to only the requested metric so JSON
/// consumers don't have to walk every field. `None` filter returns the
/// full delta unchanged.
fn filtered_delta(delta: Option<&SnapshotDelta>, metric: Option<MetricKind>) -> serde_json::Value {
    let Some(d) = delta else {
        return serde_json::Value::Null;
    };
    let Some(m) = metric else {
        return json!(d);
    };
    let mut out = serde_json::Map::new();
    if let Some(s) = d.from_sha.as_ref() {
        out.insert("from_sha".into(), json!(s));
    }
    if let Some(t) = d.from_timestamp.as_ref() {
        out.insert("from_timestamp".into(), json!(t));
    }
    match m {
        MetricKind::Loc => {} // delta has no loc payload yet
        MetricKind::Complexity => {
            out.insert("complexity".into(), json!(d.complexity));
        }
        MetricKind::Churn => {
            out.insert("churn".into(), json!(d.churn));
        }
        MetricKind::ChangeCoupling => {
            out.insert("change_coupling".into(), json!(d.change_coupling));
        }
        MetricKind::Duplication => {
            out.insert("duplication".into(), json!(d.duplication));
        }
        MetricKind::Hotspot => {
            out.insert("hotspot".into(), json!(d.hotspot));
        }
        MetricKind::Lcom => {
            // SnapshotDelta doesn't carry an LCOM diff yet; emit Null
            // so consumers see the metric was filtered through.
            out.insert("lcom".into(), serde_json::Value::Null);
        }
    }
    serde_json::Value::Object(out)
}

fn print_delta_summary(
    w: &mut dyn Write,
    prev: &Event,
    delta: &SnapshotDelta,
    metric: Option<MetricKind>,
) -> io::Result<()> {
    writeln!(w)?;
    let from_label = delta.from_sha.as_deref().map_or_else(
        || prev.timestamp.format("%Y-%m-%d").to_string(),
        |s| s.chars().take(8).collect::<String>(),
    );
    writeln!(w, "  delta vs prior snapshot ({from_label}):")?;
    if matches_metric(metric, MetricKind::Complexity) {
        if let Some(c) = delta.complexity.as_ref() {
            writeln!(
                w,
                "    complexity:  max_ccn {:+}  max_cog {:+}  fns {:+}",
                c.max_ccn, c.max_cognitive, c.functions,
            )?;
            if !c.new_top_ccn.is_empty() {
                writeln!(w, "      new in top CCN: {}", c.new_top_ccn.join(", "))?;
            }
        }
    }
    if matches_metric(metric, MetricKind::Churn) {
        if let Some(ch) = delta.churn.as_ref() {
            writeln!(
                w,
                "    churn:       commits_in_window {:+}  top_changed={}",
                ch.commits_in_window, ch.top_file_changed,
            )?;
        }
    }
    if matches_metric(metric, MetricKind::Hotspot) {
        if let Some(h) = delta.hotspot.as_ref() {
            writeln!(w, "    hotspot:     max_score {:+.1}", h.max_score)?;
            if !h.top_files_added.is_empty() {
                writeln!(w, "      added:    {}", h.top_files_added.join(", "))?;
            }
            if !h.top_files_dropped.is_empty() {
                writeln!(w, "      dropped:  {}", h.top_files_dropped.join(", "))?;
            }
        }
    }
    if matches_metric(metric, MetricKind::Duplication) {
        if let Some(d) = delta.duplication.as_ref() {
            writeln!(
                w,
                "    duplication: blocks {:+}  tokens {:+}",
                d.duplicate_blocks, d.duplicate_tokens,
            )?;
        }
    }
    if matches_metric(metric, MetricKind::ChangeCoupling) {
        if let Some(cc) = delta.change_coupling.as_ref() {
            writeln!(
                w,
                "    coupling:    pairs {:+}  files {:+}",
                cc.pairs, cc.files,
            )?;
        }
    }
    Ok(())
}
