use std::path::Path;

use anyhow::Result;
use heal_core::config::load_from_project;
use heal_core::history::HistoryReader;
use heal_core::HealPaths;
use serde_json::json;

pub fn run(project: &Path, json_output: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    let cfg_exists = paths.config().exists();
    let history_segments = HistoryReader::new(paths.history_dir()).segments()?;
    let snapshot_count = HistoryReader::iter_segments(history_segments.clone())
        .flatten()
        .count();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "initialized": cfg_exists,
                "history_segments": history_segments.len(),
                "snapshots": snapshot_count,
            }))?
        );
        return Ok(());
    }

    if !cfg_exists {
        println!("HEAL is not initialized in this project. Run `heal init` first.");
        return Ok(());
    }
    let _cfg = load_from_project(project)?;
    println!("HEAL status (project: {})", project.display());
    println!("  config:           {}", paths.config().display());
    println!("  history segments: {}", history_segments.len());
    println!("  snapshots:        {snapshot_count}");
    println!();
    println!("(metric findings: not yet implemented in v0.1 foundation)");
    Ok(())
}
