//! `heal compact` — gzip + delete old segments under
//! `.heal/{snapshots,logs,checks}/`.
//!
//! The same routine fires best-effort from `heal hook commit`, so
//! users rarely need to call this manually; it remains as a CLI
//! surface for diagnostics ("did anything actually compact?") and
//! for bulk catch-up after a long upgrade gap.

use std::path::Path;

use anyhow::Result;
use chrono::Utc;

use crate::core::compaction::{self, CompactionPolicy, CompactionStats};
use crate::core::eventlog::EventLog;
use crate::core::HealPaths;

pub fn run(project: &Path, verbose: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    let summary = run_all(&paths, &CompactionPolicy::default())?;
    print_summary(&summary, verbose);
    Ok(())
}

/// Public so `heal hook commit` can invoke compaction with the same
/// policy.
pub fn run_all(paths: &HealPaths, policy: &CompactionPolicy) -> Result<Summary> {
    let now = Utc::now();
    let mut summary = Summary::default();
    for (label, dir) in [
        ("snapshots", paths.snapshots_dir()),
        ("logs", paths.logs_dir()),
        ("checks", paths.checks_dir()),
    ] {
        let log = EventLog::new(&dir);
        summary
            .per_dir
            .push((label, compaction::compact(&log, policy, now)?));
    }
    Ok(summary)
}

#[derive(Debug, Default)]
pub struct Summary {
    pub per_dir: Vec<(&'static str, CompactionStats)>,
}

impl Summary {
    #[must_use]
    pub fn touched(&self) -> bool {
        self.per_dir.iter().any(|(_, s)| s.touched())
    }

    fn totals(&self) -> (usize, usize) {
        self.per_dir.iter().fold((0, 0), |(g, d), (_, s)| {
            (g + s.gzipped.len(), d + s.deleted.len())
        })
    }
}

fn print_summary(summary: &Summary, verbose: bool) {
    if !summary.touched() {
        println!("nothing to compact (all segments within 90 days)");
        return;
    }
    let (gzipped, deleted) = summary.totals();
    println!("compacted: {gzipped} gzipped, {deleted} deleted");
    if !verbose {
        return;
    }
    for (label, stats) in &summary.per_dir {
        if !stats.touched() {
            continue;
        }
        println!("  {label}/");
        for path in &stats.gzipped {
            println!("    gzip   {}", path.display());
        }
        for path in &stats.deleted {
            println!("    delete {}", path.display());
        }
    }
}
