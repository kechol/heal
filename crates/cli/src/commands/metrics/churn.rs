//! `churn` section — recent commits per file, with the configurable
//! `since_days` window.

use std::io::{self, Write};

use serde_json::json;

use super::section::{write_section_header, MetricSection, SectionCtx};
use crate::cli::MetricKind;

pub(super) struct ChurnSection;

impl MetricSection for ChurnSection {
    fn metric(&self) -> MetricKind {
        MetricKind::Churn
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.churn.as_ref() else {
            return Ok(());
        };
        let top_n = ctx.cfg.metrics.top_n_churn();
        write_section_header("Churn", ctx, w)?;
        if report.files.is_empty() {
            writeln!(w, "  no commits in the last {} days", report.since_days)?;
            return Ok(());
        }
        writeln!(
            w,
            "  {} files across {} commits (last {} days, +{}/-{} lines)",
            report.totals.files,
            report.totals.commits,
            report.since_days,
            report.totals.lines_added,
            report.totals.lines_deleted,
        )?;
        writeln!(w, "  most-churned files:")?;
        for f in report.worst_n(top_n) {
            writeln!(
                w,
                "    - {:>3}  {}  (+{}/-{})",
                f.commits,
                f.path.display(),
                f.lines_added,
                f.lines_deleted,
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.churn.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n_churn();
        let files = ctx
            .reports
            .churn
            .as_ref()
            .map(|r| r.worst_n(n))
            .unwrap_or_default();
        (n, json!({ "files": files }))
    }
}
