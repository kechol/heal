//! `coverage_pct` section — per-source-file line coverage from the
//! lcov.info ingested under `[features.test.coverage]`.

use std::io::{self, Write};

use serde_json::json;

use crate::cli::MetricKind;
use crate::commands::metrics::section::{write_section_header, MetricSection, SectionCtx};

pub(super) struct CoveragePctSection;

impl MetricSection for CoveragePctSection {
    fn metric(&self) -> MetricKind {
        MetricKind::CoveragePct
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.coverage.as_ref() else {
            return Ok(());
        };
        if report.entries.is_empty() {
            return Ok(());
        }
        let top_n = ctx.cfg.metrics.top_n;
        write_section_header("Coverage", ctx, w)?;
        if let Some(src) = report.source.as_ref() {
            writeln!(w, "  source: {}", src.display())?;
        }
        let uncovered = report.uncovered_count();
        writeln!(
            w,
            "  {} files with line coverage, {} below 100%",
            report.entries.len(),
            uncovered,
        )?;
        if uncovered == 0 {
            return Ok(());
        }
        writeln!(w, "  least-covered files:")?;
        for entry in report.worst_n(top_n) {
            writeln!(
                w,
                "    - {} ({:.0}% — {}/{} lines)",
                entry.path.display(),
                entry.line_coverage_pct,
                entry.lines_hit,
                entry.lines_found,
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.coverage.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n;
        let entries: Vec<_> = ctx
            .reports
            .coverage
            .as_ref()
            .map(|r| r.worst_n(n))
            .unwrap_or_default();
        (n, json!({ "entries": entries }))
    }
}
