//! `doc_freshness` section — pairs whose source side has moved past
//! the paired doc.

use std::io::{self, Write};

use serde_json::json;

use super::section::{write_section_header, MetricSection, SectionCtx};
use crate::cli::MetricKind;

pub(super) struct DocFreshnessSection;

impl MetricSection for DocFreshnessSection {
    fn metric(&self) -> MetricKind {
        MetricKind::DocFreshness
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.doc_freshness.as_ref() else {
            return Ok(());
        };
        if report.entries.is_empty() {
            return Ok(());
        }
        let top_n = ctx.cfg.metrics.top_n;
        write_section_header("Doc freshness", ctx, w)?;
        writeln!(
            w,
            "  {} pairs tracked, {} stale (high≥{} commits, critical≥{})",
            report.totals.pairs,
            report.totals.stale_pairs,
            ctx.cfg.features.docs.doc_freshness.high_commits,
            ctx.cfg.features.docs.doc_freshness.critical_commits,
        )?;
        if report.totals.stale_pairs == 0 {
            return Ok(());
        }
        writeln!(w, "  most-drifted pairs:")?;
        for entry in report.worst_n(top_n) {
            if entry.src_commits_since_doc == 0 {
                continue;
            }
            let srcs: Vec<String> = entry
                .src_paths
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            writeln!(
                w,
                "    - {} ⇐ {} ({} src commits since doc)",
                entry.doc_path.display(),
                srcs.join(", "),
                entry.src_commits_since_doc,
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.doc_freshness.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n;
        let entries = ctx
            .reports
            .doc_freshness
            .as_ref()
            .map(|r| r.worst_n(n))
            .unwrap_or_default();
        (n, json!({ "entries": entries }))
    }
}
