use std::path::Path;

use crate::core::config::load_from_project;
use crate::core::config::MetricsConfig;
use crate::core::eventlog::{Event, EventLog};
use crate::core::snapshot::{MetricsSnapshot, SnapshotDelta};
use crate::core::HealPaths;
use crate::observer::change_coupling::ChangeCouplingReport;
use crate::observer::churn::ChurnReport;
use crate::observer::complexity::{ComplexityMetric, ComplexityObserver, ComplexityReport};
use crate::observer::duplication::DuplicationReport;
use crate::observer::hotspot::HotspotReport;
use crate::observer::loc::LocReport;
use anyhow::Result;
use serde_json::json;

use crate::cli::StatusMetric;
use crate::observers::{run_all, ObserverReports};

pub fn run(project: &Path, json_output: bool, metric: Option<StatusMetric>) -> Result<()> {
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
    let reports = cfg.as_ref().map(|c| run_all(project, c, metric));

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&build_json(
                cfg_exists,
                segment_count,
                snapshot_count,
                reports.as_ref(),
                cfg.as_ref().map(|c| &c.metrics),
                delta.as_ref(),
                metric,
            ))?
        );
        return Ok(());
    }

    if !cfg_exists {
        println!("HEAL is not initialized in this project. Run `heal init` first.");
        return Ok(());
    }
    let metrics = &cfg
        .as_ref()
        .expect("cfg_exists branch implies cfg loaded")
        .metrics;
    let reports = reports.expect("cfg present implies reports built");
    println!("HEAL status (project: {})", project.display());
    println!("  config:            {}", paths.config().display());
    println!("  snapshot segments: {segment_count}");
    println!("  snapshots:         {snapshot_count}");
    if matches_metric(metric, StatusMetric::Loc) {
        print_loc_summary(&reports.loc, metrics.top_n_loc());
    }
    if matches_metric(metric, StatusMetric::Complexity) {
        print_complexity_summary(
            &reports.complexity_observer,
            &reports.complexity,
            metrics.top_n_complexity(),
        );
    }
    if matches_metric(metric, StatusMetric::Churn) {
        if let Some(report) = reports.churn.as_ref() {
            print_churn_summary(report, metrics.top_n_churn());
        }
    }
    if matches_metric(metric, StatusMetric::ChangeCoupling) {
        if let Some(report) = reports.change_coupling.as_ref() {
            print_coupling_summary(report, metrics.top_n_change_coupling());
        }
    }
    if matches_metric(metric, StatusMetric::Duplication) {
        if let Some(report) = reports.duplication.as_ref() {
            print_duplication_summary(report, metrics.top_n_duplication());
        }
    }
    if matches_metric(metric, StatusMetric::Hotspot) {
        if let Some(report) = reports.hotspot.as_ref() {
            print_hotspot_summary(report, metrics.top_n_hotspot());
        }
    }
    if let (Some((snap, _)), Some(d)) = (latest.as_ref(), delta.as_ref()) {
        print_delta_summary(snap, d, metric);
    }
    Ok(())
}

/// `None` means "no filter, print everything"; otherwise print only when
/// the section matches the requested metric.
fn matches_metric(filter: Option<StatusMetric>, section: StatusMetric) -> bool {
    filter.is_none_or(|f| f == section)
}

fn build_json(
    cfg_exists: bool,
    segment_count: usize,
    snapshot_count: usize,
    reports: Option<&ObserverReports>,
    metrics_cfg: Option<&MetricsConfig>,
    delta: Option<&SnapshotDelta>,
    metric: Option<StatusMetric>,
) -> serde_json::Value {
    let mut payload = serde_json::Map::new();
    payload.insert("initialized".into(), json!(cfg_exists));
    payload.insert("snapshot_segments".into(), json!(segment_count));
    payload.insert("snapshots".into(), json!(snapshot_count));
    if let Some(m) = metric {
        payload.insert("metric".into(), json!(m.json_key()));
    }
    // Raw reports balloon for large repos (the `worst` precomputation already
    // captures what filtered consumers need); only emit them in the unfiltered
    // path so `--metric X --json` stays lean for skill consumption.
    if metric.is_none() {
        if let Some(r) = reports {
            payload.insert("loc".into(), json!(&r.loc));
            payload.insert("complexity".into(), json!(&r.complexity));
            payload.insert("churn".into(), json!(r.churn.as_ref()));
            payload.insert("change_coupling".into(), json!(r.change_coupling.as_ref()));
            payload.insert("duplication".into(), json!(r.duplication.as_ref()));
            payload.insert("hotspot".into(), json!(r.hotspot.as_ref()));
        }
    }
    if let (Some(m), Some(reports), Some(cfg)) = (metric, reports, metrics_cfg) {
        let (top_n, worst) = build_worst(m, reports, cfg);
        payload.insert("top_n".into(), json!(top_n));
        payload.insert("worst".into(), worst);
    }
    payload.insert("delta".into(), filtered_delta(delta, metric));
    serde_json::Value::Object(payload)
}

/// Precompute the top-N "worst" view per metric using the config-driven
/// `top_n` so skills don't have to sort and slice the raw report.
/// Returns `(n, worst_value)` so callers can emit `top_n` alongside.
fn build_worst(
    metric: StatusMetric,
    reports: &ObserverReports,
    cfg: &MetricsConfig,
) -> (usize, serde_json::Value) {
    match metric {
        StatusMetric::Loc => {
            let n = cfg.top_n_loc();
            let langs: Vec<_> = reports.loc.languages.iter().take(n).collect();
            (n, json!({ "languages": langs }))
        }
        StatusMetric::Complexity => {
            let n = cfg.top_n_complexity();
            let ccn = reports.complexity.worst_n(n, ComplexityMetric::Ccn);
            let cog = reports.complexity.worst_n(n, ComplexityMetric::Cognitive);
            (n, json!({ "ccn": ccn, "cognitive": cog }))
        }
        StatusMetric::Churn => {
            let n = cfg.top_n_churn();
            let files = reports
                .churn
                .as_ref()
                .map(|r| r.worst_n(n))
                .unwrap_or_default();
            (n, json!({ "files": files }))
        }
        StatusMetric::ChangeCoupling => {
            let n = cfg.top_n_change_coupling();
            let pairs = reports
                .change_coupling
                .as_ref()
                .map(|r| r.worst_n_pairs(n))
                .unwrap_or_default();
            let files = reports
                .change_coupling
                .as_ref()
                .map(|r| r.worst_n_files(n))
                .unwrap_or_default();
            (n, json!({ "pairs": pairs, "files": files }))
        }
        StatusMetric::Duplication => {
            let n = cfg.top_n_duplication();
            let blocks = reports
                .duplication
                .as_ref()
                .map(|r| r.worst_n_blocks(n))
                .unwrap_or_default();
            (n, json!({ "blocks": blocks }))
        }
        StatusMetric::Hotspot => {
            let n = cfg.top_n_hotspot();
            let entries = reports
                .hotspot
                .as_ref()
                .map(|r| r.worst_n(n))
                .unwrap_or_default();
            (n, json!({ "entries": entries }))
        }
        StatusMetric::Lcom => {
            let n = cfg.top_n_lcom();
            let classes = reports
                .lcom
                .as_ref()
                .map(|r| r.worst_n(n))
                .unwrap_or_default();
            (n, json!({ "classes": classes }))
        }
    }
}

/// Filter the delta payload to only the requested metric so JSON
/// consumers don't have to walk every field. `None` filter returns the
/// full delta unchanged.
fn filtered_delta(
    delta: Option<&SnapshotDelta>,
    metric: Option<StatusMetric>,
) -> serde_json::Value {
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
        StatusMetric::Loc => {} // delta has no loc payload yet
        StatusMetric::Complexity => {
            out.insert("complexity".into(), json!(d.complexity));
        }
        StatusMetric::Churn => {
            out.insert("churn".into(), json!(d.churn));
        }
        StatusMetric::ChangeCoupling => {
            out.insert("change_coupling".into(), json!(d.change_coupling));
        }
        StatusMetric::Duplication => {
            out.insert("duplication".into(), json!(d.duplication));
        }
        StatusMetric::Hotspot => {
            out.insert("hotspot".into(), json!(d.hotspot));
        }
        StatusMetric::Lcom => {
            // SnapshotDelta doesn't carry an LCOM diff yet; emit Null
            // so consumers see the metric was filtered through.
            out.insert("lcom".into(), serde_json::Value::Null);
        }
    }
    serde_json::Value::Object(out)
}

fn print_delta_summary(prev: &Event, delta: &SnapshotDelta, metric: Option<StatusMetric>) {
    println!();
    let from_label = delta.from_sha.as_deref().map_or_else(
        || prev.timestamp.format("%Y-%m-%d").to_string(),
        |s| s.chars().take(8).collect::<String>(),
    );
    println!("  delta vs prior snapshot ({from_label}):");
    if matches_metric(metric, StatusMetric::Complexity) {
        if let Some(c) = delta.complexity.as_ref() {
            println!(
                "    complexity:  max_ccn {:+}  max_cog {:+}  fns {:+}",
                c.max_ccn, c.max_cognitive, c.functions,
            );
            if !c.new_top_ccn.is_empty() {
                println!("      new in top CCN: {}", c.new_top_ccn.join(", "));
            }
        }
    }
    if matches_metric(metric, StatusMetric::Churn) {
        if let Some(ch) = delta.churn.as_ref() {
            println!(
                "    churn:       commits_in_window {:+}  top_changed={}",
                ch.commits_in_window, ch.top_file_changed,
            );
        }
    }
    if matches_metric(metric, StatusMetric::Hotspot) {
        if let Some(h) = delta.hotspot.as_ref() {
            println!("    hotspot:     max_score {:+.1}", h.max_score);
            if !h.top_files_added.is_empty() {
                println!("      added:    {}", h.top_files_added.join(", "));
            }
            if !h.top_files_dropped.is_empty() {
                println!("      dropped:  {}", h.top_files_dropped.join(", "));
            }
        }
    }
    if matches_metric(metric, StatusMetric::Duplication) {
        if let Some(d) = delta.duplication.as_ref() {
            println!(
                "    duplication: blocks {:+}  tokens {:+}",
                d.duplicate_blocks, d.duplicate_tokens,
            );
        }
    }
    if matches_metric(metric, StatusMetric::ChangeCoupling) {
        if let Some(cc) = delta.change_coupling.as_ref() {
            println!(
                "    coupling:    pairs {:+}  files {:+}",
                cc.pairs, cc.files,
            );
        }
    }
}

fn print_loc_summary(report: &LocReport, top_n: usize) {
    println!();
    if let Some(name) = report.primary.as_deref() {
        println!(
            "  primary language: {name} ({} LOC, {} files total)",
            report.totals.code,
            report.total_files()
        );
    } else {
        println!("  primary language: (none detected)");
    }
    if !report.languages.is_empty() {
        println!("  top languages:");
        for entry in report.languages.iter().take(top_n) {
            println!(
                "    - {:<16} {:>6} LOC across {} files",
                entry.name, entry.counts.code, entry.files
            );
        }
    }
}

fn print_complexity_summary(obs: &ComplexityObserver, report: &ComplexityReport, top_n: usize) {
    if !obs.ccn_enabled && !obs.cognitive_enabled {
        return;
    }
    println!();
    if report.files.is_empty() {
        println!("  complexity: no supported source files found");
        return;
    }
    println!(
        "  complexity: {} functions across {} files (max CCN {}, max Cognitive {})",
        report.totals.functions,
        report.totals.files,
        report.totals.max_ccn,
        report.totals.max_cognitive,
    );
    if obs.ccn_enabled {
        print_top_functions(report, "highest CCN", ComplexityMetric::Ccn, top_n);
    }
    if obs.cognitive_enabled {
        print_top_functions(
            report,
            "highest Cognitive",
            ComplexityMetric::Cognitive,
            top_n,
        );
    }
}

fn print_top_functions(
    report: &ComplexityReport,
    header: &str,
    metric: ComplexityMetric,
    top_n: usize,
) {
    let top = report.worst_n(top_n, metric);
    if top.is_empty() {
        return;
    }
    println!("  {header}:");
    for f in &top {
        let score = match metric {
            ComplexityMetric::Ccn => f.ccn,
            ComplexityMetric::Cognitive => f.cognitive,
        };
        println!(
            "    - {:>3}  {}:{:<4}  {}",
            score,
            f.file.display(),
            f.line,
            f.name,
        );
    }
}

fn print_churn_summary(report: &ChurnReport, top_n: usize) {
    println!();
    if report.files.is_empty() {
        println!("  churn: no commits in the last {} days", report.since_days);
        return;
    }
    println!(
        "  churn (last {} days): {} files across {} commits (+{}/-{} lines)",
        report.since_days,
        report.totals.files,
        report.totals.commits,
        report.totals.lines_added,
        report.totals.lines_deleted,
    );
    println!("  most-churned files:");
    for f in report.worst_n(top_n) {
        println!(
            "    - {:>3}  {}  (+{}/-{})",
            f.commits,
            f.path.display(),
            f.lines_added,
            f.lines_deleted,
        );
    }
}

fn print_coupling_summary(report: &ChangeCouplingReport, top_n: usize) {
    println!();
    if report.pairs.is_empty() {
        println!(
            "  change coupling: no pairs at min_coupling={} ({} commits scanned)",
            report.min_coupling, report.totals.commits_considered,
        );
        return;
    }
    println!(
        "  change coupling: {} pairs across {} files (min_coupling={}, {} commits scanned)",
        report.totals.pairs,
        report.totals.files,
        report.min_coupling,
        report.totals.commits_considered,
    );
    println!("  most-coupled pairs:");
    for pair in report.worst_n_pairs(top_n) {
        println!(
            "    - {:>3}  {}  ↔  {}",
            pair.count,
            pair.a.display(),
            pair.b.display(),
        );
    }
}

fn print_duplication_summary(report: &DuplicationReport, top_n: usize) {
    println!();
    if report.blocks.is_empty() {
        println!(
            "  duplication: no blocks ≥ {} tokens detected",
            report.min_tokens
        );
        return;
    }
    println!(
        "  duplication: {} blocks affecting {} files (min_tokens={}, total duplicate tokens {})",
        report.totals.duplicate_blocks,
        report.totals.files_affected,
        report.min_tokens,
        report.totals.duplicate_tokens,
    );
    println!("  largest duplicate blocks:");
    for block in report.worst_n_blocks(top_n) {
        let locs: Vec<String> = block
            .locations
            .iter()
            .map(|l| format!("{}:{}-{}", l.path.display(), l.start_line, l.end_line))
            .collect();
        println!(
            "    - {:>3} tokens × {} locations: {}",
            block.token_count,
            block.locations.len(),
            locs.join(", "),
        );
    }
}

fn print_hotspot_summary(report: &HotspotReport, top_n: usize) {
    println!();
    if report.entries.is_empty() {
        println!("  hotspot: no files have both churn and complexity signal");
        return;
    }
    println!(
        "  hotspot: {} files (max score {:.1})",
        report.totals.files, report.totals.max_score,
    );
    println!("  top hotspots (CCN_sum × commits):");
    for entry in report.worst_n(top_n) {
        println!(
            "    - {:>6.1}  {}  (CCN_sum={}, commits={})",
            entry.score,
            entry.path.display(),
            entry.ccn_sum,
            entry.churn_commits,
        );
    }
}
