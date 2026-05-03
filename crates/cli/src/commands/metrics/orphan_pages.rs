//! `orphan_pages` section — Layer B docs nothing else links to.

use std::io::{self, Write};

use serde_json::json;

use super::section::{write_section_header, MetricSection, SectionCtx};
use crate::cli::MetricKind;

pub(super) struct OrphanPagesSection;

impl MetricSection for OrphanPagesSection {
    fn metric(&self) -> MetricKind {
        MetricKind::OrphanPages
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.orphan_pages.as_ref() else {
            return Ok(());
        };
        if report.totals.scanned_docs == 0 {
            return Ok(());
        }
        let top_n = ctx.cfg.metrics.top_n;
        write_section_header("Orphan pages", ctx, w)?;
        writeln!(
            w,
            "  {} doc(s) scanned; {} not linked from anywhere",
            report.totals.scanned_docs, report.totals.orphans,
        )?;
        for path in report.worst_n(top_n) {
            writeln!(w, "    - {}", path.display())?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.orphan_pages.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n;
        let orphans = ctx
            .reports
            .orphan_pages
            .as_ref()
            .map(|r| r.worst_n(n))
            .unwrap_or_default();
        (n, json!({ "orphans": orphans }))
    }
}
