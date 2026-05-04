//! `doc_hotspot` section — per-pair `paired_src_churn × debt`
//! composite. Docs-family analogue of code Hotspot.

use std::io::{self, Write};

use serde_json::json;

use crate::cli::MetricKind;
use crate::commands::metrics::section::{write_section_header, MetricSection, SectionCtx};

pub(super) struct DocHotspotSection;

impl MetricSection for DocHotspotSection {
    fn metric(&self) -> MetricKind {
        MetricKind::DocHotspot
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.doc_hotspot.as_ref() else {
            return Ok(());
        };
        let top_n = ctx
            .cfg
            .features
            .docs
            .hotspot
            .top_n
            .unwrap_or(ctx.cfg.metrics.top_n);
        write_section_header("Doc hotspot", ctx, w)?;
        if report.entries.is_empty() {
            writeln!(
                w,
                "  no paired docs combine paired-src churn with measurable debt"
            )?;
            return Ok(());
        }
        writeln!(
            w,
            "  {} pairs (max score={:.1})",
            report.totals.pairs, report.totals.max_score,
        )?;
        writeln!(w, "  top doc-hotspots (paired_src_churn × debt):")?;
        for entry in report.worst_n(top_n) {
            writeln!(
                w,
                "    - {:>6.1}  {}  (src_churn={}, since_doc={}, dangling={})",
                entry.score,
                entry.doc_path.display(),
                entry.paired_src_churn,
                entry.src_commits_since_doc,
                entry.dangling_idents,
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.doc_hotspot.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx
            .cfg
            .features
            .docs
            .hotspot
            .top_n
            .unwrap_or(ctx.cfg.metrics.top_n);
        let entries = ctx
            .reports
            .doc_hotspot
            .as_ref()
            .map(|r| r.worst_n(n))
            .unwrap_or_default();
        (n, json!({ "entries": entries }))
    }
}
