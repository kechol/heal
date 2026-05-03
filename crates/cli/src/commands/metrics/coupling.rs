//! `change_coupling` section — file pairs that consistently co-change.

use std::io::{self, Write};

use serde_json::json;

use super::section::{write_section_header, MetricSection, SectionCtx};
use crate::cli::MetricKind;

pub(super) struct ChangeCouplingSection;

impl MetricSection for ChangeCouplingSection {
    fn metric(&self) -> MetricKind {
        MetricKind::ChangeCoupling
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.change_coupling.as_ref() else {
            return Ok(());
        };
        let top_n = ctx.cfg.metrics.top_n_change_coupling();
        write_section_header("Change Coupling", ctx, w)?;
        if report.pairs.is_empty() {
            writeln!(
                w,
                "  no pairs at min_coupling={} ({} commits scanned)",
                report.min_coupling, report.totals.commits_considered,
            )?;
            return Ok(());
        }
        writeln!(
            w,
            "  {} pairs across {} files (min_coupling={}, {} commits scanned)",
            report.totals.pairs,
            report.totals.files,
            report.min_coupling,
            report.totals.commits_considered,
        )?;
        writeln!(w, "  most-coupled pairs:")?;
        for pair in report.worst_n_pairs(top_n) {
            writeln!(
                w,
                "    - {:>3}  {}  ↔  {}",
                pair.count,
                pair.a.display(),
                pair.b.display(),
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.change_coupling.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n_change_coupling();
        let pairs = ctx
            .reports
            .change_coupling
            .as_ref()
            .map(|r| r.worst_n_pairs(n))
            .unwrap_or_default();
        let files = ctx
            .reports
            .change_coupling
            .as_ref()
            .map(|r| r.worst_n_files(n))
            .unwrap_or_default();
        (n, json!({ "pairs": pairs, "files": files }))
    }
}
