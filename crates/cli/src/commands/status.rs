use std::path::Path;

use anyhow::Result;
use heal_core::config::load_from_project;
use heal_core::history::HistoryReader;
use heal_core::HealPaths;
use heal_observer::complexity::{ComplexityMetric, ComplexityObserver, ComplexityReport};
use heal_observer::loc::{LocObserver, LocReport};
use serde_json::json;

/// Number of language entries to show inline in the text-mode summary.
const TOP_LANGUAGES: usize = 5;
/// Number of high-complexity functions to surface per metric.
const TOP_FUNCTIONS: usize = 5;

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

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "initialized": cfg_exists,
                "history_segments": history_segments.len(),
                "snapshots": snapshot_count,
                "loc": loc,
                "complexity": complexity,
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
