use std::path::Path;

use anyhow::Result;
use heal_core::config::load_from_project;
use heal_core::history::HistoryReader;
use heal_core::HealPaths;
use heal_observer::change_coupling::{ChangeCouplingObserver, ChangeCouplingReport};
use heal_observer::churn::{ChurnObserver, ChurnReport};
use heal_observer::complexity::{ComplexityMetric, ComplexityObserver, ComplexityReport};
use heal_observer::duplication::{DuplicationObserver, DuplicationReport};
use heal_observer::hotspot::{compose as compose_hotspot, HotspotReport, HotspotWeights};
use heal_observer::loc::{LocObserver, LocReport};
use serde_json::json;

/// Number of language entries to show inline in the text-mode summary.
const TOP_LANGUAGES: usize = 5;
/// Number of high-complexity functions to surface per metric.
const TOP_FUNCTIONS: usize = 5;
/// Number of files to surface for churn / coupling / hotspot rankings.
const TOP_FILES: usize = 5;
/// Number of duplication blocks to surface inline.
const TOP_DUPLICATES: usize = 5;

pub fn run(project: &Path, json_output: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    let cfg_exists = paths.config().exists();
    let history_segments = HistoryReader::new(paths.history_dir()).segments()?;
    let snapshot_count = HistoryReader::iter_segments(history_segments.clone())
        .flatten()
        .count();

    let cfg = if cfg_exists {
        Some(load_from_project(project)?)
    } else {
        None
    };
    let loc = cfg
        .as_ref()
        .map(|c| LocObserver::from_config(c).scan(project));
    let complexity_observer = cfg.as_ref().map(ComplexityObserver::from_config);
    let complexity = complexity_observer.as_ref().map(|obs| obs.scan(project));
    let churn = cfg
        .as_ref()
        .filter(|c| c.metrics.churn.enabled)
        .map(|c| ChurnObserver::from_config(c).scan(project));
    let change_coupling = cfg
        .as_ref()
        .filter(|c| c.metrics.change_coupling.enabled)
        .map(|c| ChangeCouplingObserver::from_config(c).scan(project));
    let duplication = cfg
        .as_ref()
        .filter(|c| c.metrics.duplication.enabled)
        .map(|c| DuplicationObserver::from_config(c).scan(project));
    let hotspot = match (cfg.as_ref(), churn.as_ref(), complexity.as_ref()) {
        (Some(c), Some(ch), Some(cx)) if c.metrics.hotspot.enabled => Some(compose_hotspot(
            ch,
            cx,
            HotspotWeights {
                churn: c.metrics.hotspot.weight_churn,
                complexity: c.metrics.hotspot.weight_complexity,
            },
        )),
        _ => None,
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "initialized": cfg_exists,
                "history_segments": history_segments.len(),
                "snapshots": snapshot_count,
                "loc": loc,
                "complexity": complexity,
                "churn": churn,
                "change_coupling": change_coupling,
                "duplication": duplication,
                "hotspot": hotspot,
            }))?
        );
        return Ok(());
    }

    if !cfg_exists {
        println!("HEAL is not initialized in this project. Run `heal init` first.");
        return Ok(());
    }
    println!("HEAL status (project: {})", project.display());
    println!("  config:           {}", paths.config().display());
    println!("  history segments: {}", history_segments.len());
    println!("  snapshots:        {snapshot_count}");
    if let Some(report) = loc.as_ref() {
        print_loc_summary(report);
    }
    if let (Some(obs), Some(report)) = (complexity_observer.as_ref(), complexity.as_ref()) {
        print_complexity_summary(obs, report);
    }
    if let Some(report) = churn.as_ref() {
        print_churn_summary(report);
    }
    if let Some(report) = change_coupling.as_ref() {
        print_coupling_summary(report);
    }
    if let Some(report) = duplication.as_ref() {
        print_duplication_summary(report);
    }
    if let Some(report) = hotspot.as_ref() {
        print_hotspot_summary(report);
    }
    Ok(())
}

fn print_loc_summary(report: &LocReport) {
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
        for entry in report.languages.iter().take(TOP_LANGUAGES) {
            println!(
                "    - {:<16} {:>6} LOC across {} files",
                entry.name, entry.counts.code, entry.files
            );
        }
    }
}

fn print_complexity_summary(obs: &ComplexityObserver, report: &ComplexityReport) {
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
        print_top_functions(report, "highest CCN", ComplexityMetric::Ccn);
    }
    if obs.cognitive_enabled {
        print_top_functions(report, "highest Cognitive", ComplexityMetric::Cognitive);
    }
}

fn print_top_functions(report: &ComplexityReport, header: &str, metric: ComplexityMetric) {
    let top = report.worst_n(TOP_FUNCTIONS, metric);
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

fn print_churn_summary(report: &ChurnReport) {
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
    for f in report.worst_n(TOP_FILES) {
        println!(
            "    - {:>3}  {}  (+{}/-{})",
            f.commits,
            f.path.display(),
            f.lines_added,
            f.lines_deleted,
        );
    }
}

fn print_coupling_summary(report: &ChangeCouplingReport) {
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
    for pair in report.pairs.iter().take(TOP_FILES) {
        println!(
            "    - {:>3}  {}  ↔  {}",
            pair.count,
            pair.a.display(),
            pair.b.display(),
        );
    }
}

fn print_duplication_summary(report: &DuplicationReport) {
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
    for block in report.blocks.iter().take(TOP_DUPLICATES) {
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

fn print_hotspot_summary(report: &HotspotReport) {
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
    for entry in report.worst_n(TOP_FILES) {
        println!(
            "    - {:>6.1}  {}  (CCN_sum={}, commits={})",
            entry.score,
            entry.path.display(),
            entry.ccn_sum,
            entry.churn_commits,
        );
    }
}
