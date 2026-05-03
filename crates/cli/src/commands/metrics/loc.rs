//! `loc` section — primary language + per-language LOC + file counts.

use std::io::{self, Write};

use serde_json::json;

use super::section::{write_section_header, MetricSection, SectionCtx};
use crate::cli::MetricKind;

pub(super) struct LocSection;

impl MetricSection for LocSection {
    fn metric(&self) -> MetricKind {
        MetricKind::Loc
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let report = &ctx.reports.loc;
        let top_n = ctx.cfg.metrics.top_n_loc();
        write_section_header("LOC", ctx, w)?;
        writeln!(
            w,
            "  {} LOC across {} files",
            report.totals.code,
            report.total_files(),
        )?;
        if let Some(name) = report.primary.as_deref() {
            writeln!(w, "  primary language: {name}")?;
        } else {
            writeln!(w, "  primary language: (none detected)")?;
        }
        if !report.languages.is_empty() {
            writeln!(w, "  top languages:")?;
            for entry in report.languages.iter().take(top_n) {
                writeln!(
                    w,
                    "    - {:<16} {:>6} LOC across {} files",
                    entry.name, entry.counts.code, entry.files,
                )?;
            }
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(&ctx.reports.loc)
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n_loc();
        let langs: Vec<_> = ctx.reports.loc.languages.iter().take(n).collect();
        (n, json!({ "languages": langs }))
    }
}
