//! `duplication` section — clone blocks above the configured token floor.

use std::io::{self, Write};

use serde_json::json;

use super::section::{write_section_header, MetricSection, SectionCtx};
use crate::cli::MetricKind;

pub(super) struct DuplicationSection;

impl MetricSection for DuplicationSection {
    fn metric(&self) -> MetricKind {
        MetricKind::Duplication
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.duplication.as_ref() else {
            return Ok(());
        };
        let top_n = ctx.cfg.metrics.top_n_duplication();
        write_section_header("Duplication", ctx, w)?;
        if report.blocks.is_empty() {
            writeln!(w, "  no blocks ≥ {} tokens detected", report.min_tokens)?;
            return Ok(());
        }
        writeln!(
            w,
            "  {} blocks across {} files (min_tokens={}, {} duplicate tokens total)",
            report.totals.duplicate_blocks,
            report.totals.files_affected,
            report.min_tokens,
            report.totals.duplicate_tokens,
        )?;
        writeln!(w, "  largest duplicate blocks:")?;
        for block in report.worst_n_blocks(top_n) {
            let locs: Vec<String> = block
                .locations
                .iter()
                .map(|l| format!("{}:{}-{}", l.path.display(), l.start_line, l.end_line))
                .collect();
            writeln!(
                w,
                "    - {:>3} tokens × {} locations: {}",
                block.token_count,
                block.locations.len(),
                locs.join(", "),
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.duplication.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n_duplication();
        let blocks = ctx
            .reports
            .duplication
            .as_ref()
            .map(|r| r.worst_n_blocks(n))
            .unwrap_or_default();
        (n, json!({ "blocks": blocks }))
    }
}
