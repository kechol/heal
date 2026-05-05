//! `skip_ratio` section — per-test-file skipped-test ratio detected via
//! tree-sitter walks under `[features.test]`.

use std::io::{self, Write};

use serde_json::json;

use crate::cli::MetricKind;
use crate::commands::metrics::section::{write_section_header, MetricSection, SectionCtx};

pub(super) struct SkipRatioSection;

impl MetricSection for SkipRatioSection {
    fn metric(&self) -> MetricKind {
        MetricKind::SkipRatio
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.skip_ratio.as_ref() else {
            return Ok(());
        };
        if report.entries.is_empty() {
            return Ok(());
        }
        let top_n = ctx.cfg.metrics.top_n;
        write_section_header("Skip ratio", self.metric(), ctx, w)?;
        let skipped_files = report.skipped_file_count();
        writeln!(
            w,
            "  {} test files scanned, {} with at least one skipped test",
            report.entries.len(),
            skipped_files,
        )?;
        if skipped_files == 0 {
            return Ok(());
        }
        writeln!(w, "  highest skip rates:")?;
        for entry in report.worst_n(top_n) {
            writeln!(
                w,
                "    - {} ({:.0}% — {}/{} tests skipped)",
                entry.path.display(),
                entry.skip_pct,
                entry.skipped_tests,
                entry.total_tests,
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.skip_ratio.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n;
        let entries: Vec<_> = ctx
            .reports
            .skip_ratio
            .as_ref()
            .map(|r| r.worst_n(n))
            .unwrap_or_default();
        (n, json!({ "entries": entries }))
    }
}
