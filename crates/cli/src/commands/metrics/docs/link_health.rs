//! `doc_link_health` section — broken internal links across docs.

use std::io::{self, Write};

use serde_json::json;

use crate::cli::MetricKind;
use crate::commands::metrics::section::{write_section_header, MetricSection, SectionCtx};

pub(super) struct DocLinkHealthSection;

impl MetricSection for DocLinkHealthSection {
    fn metric(&self) -> MetricKind {
        MetricKind::DocLinkHealth
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.doc_link_health.as_ref() else {
            return Ok(());
        };
        if report.totals.scanned_links == 0 {
            return Ok(());
        }
        let top_n = ctx.cfg.metrics.top_n;
        write_section_header("Doc link health", self.metric(), ctx, w)?;
        writeln!(
            w,
            "  scanned {} link(s) across {} doc(s); {} broken",
            report.totals.scanned_links, report.totals.scanned_docs, report.totals.broken,
        )?;
        for entry in report.worst_n(top_n) {
            writeln!(
                w,
                "    - {}:{} → {}",
                entry.doc_path.display(),
                entry.line,
                entry.target,
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.doc_link_health.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n;
        let entries = ctx
            .reports
            .doc_link_health
            .as_ref()
            .map(|r| r.worst_n(n))
            .unwrap_or_default();
        (n, json!({ "entries": entries }))
    }
}
