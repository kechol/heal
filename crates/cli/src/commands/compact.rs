//! `heal compact` — gzip + delete old segments under
//! `.heal/{snapshots,logs,checks}/`.
//!
//! Thin wrapper around [`core::compaction::compact_all`] that
//! formats the per-dir result. The same routine fires best-effort
//! from `heal hook commit`, so manual runs are mostly for diagnostics
//! and bulk catch-up after a long upgrade gap.

use std::path::Path;

use anyhow::Result;
use chrono::Utc;

use crate::core::compaction::{compact_all, CompactionPolicy, CompactionStats};
use crate::core::HealPaths;

pub fn run(project: &Path, verbose: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    let per_dir = compact_all(&paths, &CompactionPolicy::default(), Utc::now())?;
    print_summary(&per_dir, verbose);
    Ok(())
}

fn print_summary(per_dir: &[(&'static str, CompactionStats)], verbose: bool) {
    let (gzipped, deleted) = per_dir.iter().fold((0, 0), |(g, d), (_, s)| {
        (g + s.gzipped.len(), d + s.deleted.len())
    });
    if gzipped == 0 && deleted == 0 {
        println!("nothing to compact (all segments within 90 days)");
        return;
    }
    println!("compacted: {gzipped} gzipped, {deleted} deleted");
    if !verbose {
        return;
    }
    for (label, stats) in per_dir {
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
