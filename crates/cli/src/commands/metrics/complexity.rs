//! `complexity` section — CCN + Cognitive top functions, per file.

use std::io::{self, Write};

use serde_json::json;

use super::section::{write_section_header, MetricSection, SectionCtx};
use crate::cli::MetricKind;
use crate::observer::complexity::{ComplexityMetric, ComplexityReport};

pub(super) struct ComplexitySection;

impl MetricSection for ComplexitySection {
    fn metric(&self) -> MetricKind {
        MetricKind::Complexity
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let obs = &ctx.reports.complexity_observer;
        let report = &ctx.reports.complexity;
        let top_n = ctx.cfg.metrics.top_n_complexity();
        if !obs.ccn_enabled && !obs.cognitive_enabled {
            return Ok(());
        }
        write_section_header("Complexity", ctx, w)?;
        if report.files.is_empty() {
            writeln!(w, "  no supported source files found")?;
            return Ok(());
        }
        writeln!(
            w,
            "  {} functions across {} files (max CCN={}, max Cognitive={})",
            report.totals.functions,
            report.totals.files,
            report.totals.max_ccn,
            report.totals.max_cognitive,
        )?;
        if obs.ccn_enabled {
            print_top(w, report, "highest CCN", ComplexityMetric::Ccn, top_n)?;
        }
        if obs.cognitive_enabled {
            print_top(
                w,
                report,
                "highest Cognitive",
                ComplexityMetric::Cognitive,
                top_n,
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(&ctx.reports.complexity)
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n_complexity();
        let ccn = ctx.reports.complexity.worst_n(n, ComplexityMetric::Ccn);
        let cog = ctx
            .reports
            .complexity
            .worst_n(n, ComplexityMetric::Cognitive);
        (n, json!({ "ccn": ccn, "cognitive": cog }))
    }
}

fn print_top(
    w: &mut dyn Write,
    report: &ComplexityReport,
    header: &str,
    metric: ComplexityMetric,
    top_n: usize,
) -> io::Result<()> {
    let top = report.worst_n(top_n, metric);
    if top.is_empty() {
        return Ok(());
    }
    writeln!(w, "  {header}:")?;
    for f in &top {
        let score = match metric {
            ComplexityMetric::Ccn => f.ccn,
            ComplexityMetric::Cognitive => f.cognitive,
        };
        writeln!(
            w,
            "    - {:>3}  {}:{:<4}  {}",
            score,
            f.file.display(),
            f.line,
            f.name,
        )?;
    }
    Ok(())
}
