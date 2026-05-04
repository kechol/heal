//! `test_hotspot` section — per-src-file `commits × uncov_pct`
//! composite. Test-family analogue of code Hotspot.

use std::io::{self, Write};

use serde_json::json;

use crate::cli::MetricKind;
use crate::commands::metrics::section::{write_section_header, MetricSection, SectionCtx};

pub(super) struct TestHotspotSection;

impl MetricSection for TestHotspotSection {
    fn metric(&self) -> MetricKind {
        MetricKind::TestHotspot
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.test_hotspot.as_ref() else {
            return Ok(());
        };
        let top_n = ctx
            .cfg
            .features
            .test
            .hotspot
            .top_n
            .unwrap_or(ctx.cfg.metrics.top_n);
        write_section_header("Test hotspot", ctx, w)?;
        if report.entries.is_empty() {
            writeln!(w, "  no files have both churn and a coverage gap")?;
            return Ok(());
        }
        writeln!(
            w,
            "  {} files (max score={:.1})",
            report.totals.files, report.totals.max_score,
        )?;
        writeln!(w, "  top test-hotspots (commits × uncov_pct):")?;
        for entry in report.worst_n(top_n) {
            writeln!(
                w,
                "    - {:>6.1}  {}  (uncov={:.0}%, commits={})",
                entry.score,
                entry.path.display(),
                entry.uncov_pct,
                entry.churn_commits,
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.test_hotspot.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx
            .cfg
            .features
            .test
            .hotspot
            .top_n
            .unwrap_or(ctx.cfg.metrics.top_n);
        let entries = ctx
            .reports
            .test_hotspot
            .as_ref()
            .map(|r| r.worst_n(n))
            .unwrap_or_default();
        (n, json!({ "entries": entries }))
    }
}
