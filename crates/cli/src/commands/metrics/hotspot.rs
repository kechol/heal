//! `hotspot` section — files combining high churn with high complexity.

use std::io::{self, Write};

use serde_json::json;

use super::section::{write_section_header, MetricSection, SectionCtx};
use crate::cli::MetricKind;

pub(super) struct HotspotSection;

impl MetricSection for HotspotSection {
    fn metric(&self) -> MetricKind {
        MetricKind::Hotspot
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.hotspot.as_ref() else {
            return Ok(());
        };
        let top_n = ctx.cfg.metrics.top_n_hotspot();
        write_section_header("Hotspot", ctx, w)?;
        if report.entries.is_empty() {
            writeln!(w, "  no files have both churn and complexity signal")?;
            return Ok(());
        }
        writeln!(
            w,
            "  {} files (max score={:.1})",
            report.totals.files, report.totals.max_score,
        )?;
        writeln!(w, "  top hotspots (CCN_sum × commits):")?;
        for entry in report.worst_n(top_n) {
            writeln!(
                w,
                "    - {:>6.1}  {}  (CCN_sum={}, commits={})",
                entry.score,
                entry.path.display(),
                entry.ccn_sum,
                entry.churn_commits,
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.hotspot.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n_hotspot();
        let entries = ctx
            .reports
            .hotspot
            .as_ref()
            .map(|r| r.worst_n(n))
            .unwrap_or_default();
        (n, json!({ "entries": entries }))
    }
}
