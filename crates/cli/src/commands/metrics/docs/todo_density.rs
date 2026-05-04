//! `todo_density` section — author-confessed incompleteness markers.

use std::io::{self, Write};

use serde_json::json;

use crate::cli::MetricKind;
use crate::commands::metrics::section::{write_section_header, MetricSection, SectionCtx};

pub(super) struct TodoDensitySection;

impl MetricSection for TodoDensitySection {
    fn metric(&self) -> MetricKind {
        MetricKind::TodoDensity
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.todo_density.as_ref() else {
            return Ok(());
        };
        if report.totals.scanned_docs == 0 {
            return Ok(());
        }
        let top_n = ctx.cfg.metrics.top_n;
        write_section_header("TODO density", ctx, w)?;
        writeln!(
            w,
            "  {} doc(s) scanned; {} carry markers ({} markers total)",
            report.totals.scanned_docs,
            report.totals.docs_with_markers,
            report.totals.total_markers,
        )?;
        for entry in report.worst_n(top_n) {
            writeln!(
                w,
                "    - {} ({} markers)",
                entry.doc_path.display(),
                entry.marker_count,
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.todo_density.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n;
        let entries = ctx
            .reports
            .todo_density
            .as_ref()
            .map(|r| r.worst_n(n))
            .unwrap_or_default();
        (n, json!({ "entries": entries }))
    }
}
