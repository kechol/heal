//! `doc_drift` section — dangling identifier mentions in paired docs.

use std::io::{self, Write};

use serde_json::json;

use crate::cli::MetricKind;
use crate::commands::metrics::section::{write_section_header, MetricSection, SectionCtx};

pub(super) struct DocDriftSection;

impl MetricSection for DocDriftSection {
    fn metric(&self) -> MetricKind {
        MetricKind::DocDrift
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.doc_drift.as_ref() else {
            return Ok(());
        };
        if report.entries.is_empty() {
            return Ok(());
        }
        let top_n = ctx.cfg.metrics.top_n;
        write_section_header("Doc drift", self.metric(), ctx, w)?;
        writeln!(
            w,
            "  {} dangling identifier mention(s) across paired docs",
            report.totals.dangling_identifiers,
        )?;
        for entry in report.worst_n(top_n) {
            writeln!(
                w,
                "    - {}:{} `{}` (no longer in paired src)",
                entry.doc_path.display(),
                entry.doc_line,
                entry.identifier,
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.doc_drift.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n;
        let entries = ctx
            .reports
            .doc_drift
            .as_ref()
            .map(|r| r.worst_n(n))
            .unwrap_or_default();
        (n, json!({ "entries": entries }))
    }
}
