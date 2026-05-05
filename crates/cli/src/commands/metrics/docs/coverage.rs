//! `doc_coverage` section — paired srcs whose doc is missing from disk.

use std::io::{self, Write};

use serde_json::json;

use crate::cli::MetricKind;
use crate::commands::metrics::section::{write_section_header, MetricSection, SectionCtx};

pub(super) struct DocCoverageSection;

impl MetricSection for DocCoverageSection {
    fn metric(&self) -> MetricKind {
        MetricKind::DocCoverage
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.doc_coverage.as_ref() else {
            return Ok(());
        };
        if report.totals.tracked_srcs == 0 {
            return Ok(());
        }
        let top_n = ctx.cfg.metrics.top_n;
        write_section_header("Doc coverage", self.metric(), ctx, w)?;
        writeln!(
            w,
            "  {} src files tracked, {} missing paired docs",
            report.totals.tracked_srcs, report.totals.missing_docs,
        )?;
        if report.missing.is_empty() {
            return Ok(());
        }
        for entry in report.worst_n(top_n) {
            writeln!(
                w,
                "    - {} ⇐ {} (missing)",
                entry.src_path.display(),
                entry.expected_doc_path.display(),
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.doc_coverage.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n;
        let entries = ctx
            .reports
            .doc_coverage
            .as_ref()
            .map(|r| r.worst_n(n))
            .unwrap_or_default();
        (n, json!({ "missing": entries }))
    }
}
