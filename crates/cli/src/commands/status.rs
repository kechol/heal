use std::path::Path;

use anyhow::Result;
use heal_core::config::load_from_project;
use heal_core::eventlog::{Event, EventLog};
use heal_core::snapshot::{MetricsSnapshot, SnapshotDelta};
use heal_core::HealPaths;
use heal_observer::change_coupling::ChangeCouplingReport;
use heal_observer::churn::ChurnReport;
use heal_observer::complexity::{ComplexityMetric, ComplexityObserver, ComplexityReport};
use heal_observer::duplication::DuplicationReport;
use heal_observer::hotspot::HotspotReport;
use heal_observer::loc::LocReport;
use serde_json::json;

use crate::observers::run_all;

pub fn run(project: &Path, json_output: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    let cfg_exists = paths.config().exists();
    let snapshot_segments = EventLog::new(paths.snapshots_dir()).segments()?;
    let segment_count = snapshot_segments.len();
    let snapshot_count = EventLog::iter_segments(snapshot_segments.clone())
        .flatten()
        .count();
    // Move (not clone) the segment list into `latest_in_segments` — `iter_segments`
    // above already consumed a clone, so this read is the last user.
    let latest = MetricsSnapshot::latest_in_segments(snapshot_segments).unwrap_or(None);
    let delta = latest
        .as_ref()
        .and_then(|(_, m)| m.delta.as_ref())
        .and_then(|v| serde_json::from_value::<SnapshotDelta>(v.clone()).ok());

    let cfg = if cfg_exists {
        Some(load_from_project(project)?)
    } else {
        None
    };
    let reports = cfg.as_ref().map(|c| run_all(project, c));

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "initialized": cfg_exists,
                "snapshot_segments": segment_count,
                "snapshots": snapshot_count,
                "loc": reports.as_ref().map(|r| &r.loc),
                "complexity": reports.as_ref().map(|r| &r.complexity),
                "churn": reports.as_ref().and_then(|r| r.churn.as_ref()),
                "change_coupling": reports.as_ref().and_then(|r| r.change_coupling.as_ref()),
                "duplication": reports.as_ref().and_then(|r| r.duplication.as_ref()),
                "hotspot": reports.as_ref().and_then(|r| r.hotspot.as_ref()),
                "delta": delta,
            }))?
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
    print_loc_summary(&reports.loc, metrics.top_n_loc());
    print_complexity_summary(
        &reports.complexity_observer,
        &reports.complexity,
        metrics.top_n_complexity(),
    );
    if let Some(report) = reports.churn.as_ref() {
        print_churn_summary(report, metrics.top_n_churn());
    }
    if let Some(report) = reports.change_coupling.as_ref() {
        print_coupling_summary(report, metrics.top_n_change_coupling());
    }
    if let Some(report) = reports.duplication.as_ref() {
        print_duplication_summary(report, metrics.top_n_duplication());
    }
    if let Some(report) = reports.hotspot.as_ref() {
        print_hotspot_summary(report, metrics.top_n_hotspot());
    }
    if let (Some((snap, _)), Some(d)) = (latest.as_ref(), delta.as_ref()) {
        print_delta_summary(snap, d);
    }
    Ok(())
}

fn print_delta_summary(prev: &Event, delta: &SnapshotDelta) {
    println!();
    let from_label = delta.from_sha.as_deref().map_or_else(
        || prev.timestamp.format("%Y-%m-%d").to_string(),
        |s| s.chars().take(8).collect::<String>(),
    );
    println!("  delta vs prior snapshot ({from_label}):");
    if let Some(c) = delta.complexity.as_ref() {
        println!(
            "    complexity:  max_ccn {:+}  max_cog {:+}  fns {:+}",
            c.max_ccn, c.max_cognitive, c.functions,
        );
        if !c.new_top_ccn.is_empty() {
            println!("      new in top CCN: {}", c.new_top_ccn.join(", "));
        }
    }
    if let Some(ch) = delta.churn.as_ref() {
        println!(
            "    churn:       commits_in_window {:+}  top_changed={}",
            ch.commits_in_window, ch.top_file_changed,
        );
    }
    if let Some(h) = delta.hotspot.as_ref() {
        println!("    hotspot:     max_score {:+.1}", h.max_score);
        if !h.top_files_added.is_empty() {
            println!("      added:    {}", h.top_files_added.join(", "));
        }
        if !h.top_files_dropped.is_empty() {
            println!("      dropped:  {}", h.top_files_dropped.join(", "));
        }
    }
    if let Some(d) = delta.duplication.as_ref() {
        println!(
            "    duplication: blocks {:+}  tokens {:+}",
            d.duplicate_blocks, d.duplicate_tokens,
        );
    }
    if let Some(cc) = delta.change_coupling.as_ref() {
        println!(
            "    coupling:    pairs {:+}  files {:+}",
            cc.pairs, cc.files,
        );
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
