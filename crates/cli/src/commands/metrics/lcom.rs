//! `lcom` section — Lack-of-Cohesion-of-Methods clusters per class.

use std::io::{self, Write};

use serde_json::json;

use super::section::{write_section_header, MetricSection, SectionCtx};
use crate::cli::MetricKind;

pub(super) struct LcomSection;

impl MetricSection for LcomSection {
    fn metric(&self) -> MetricKind {
        MetricKind::Lcom
    }

    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()> {
        let Some(report) = ctx.reports.lcom.as_ref() else {
            return Ok(());
        };
        let top_n = ctx.cfg.metrics.top_n_lcom();
        write_section_header("LCOM", ctx, w)?;
        if report.classes.is_empty() {
            writeln!(
                w,
                "  no classes scanned (supported: TS / JS / Python / Rust)",
            )?;
            return Ok(());
        }
        writeln!(
            w,
            "  {} classes ≥ min_cluster_count={} across {} scanned (max clusters={})",
            report.totals.classes_with_lcom,
            report.min_cluster_count,
            report.totals.classes_scanned,
            report.totals.max_cluster_count,
        )?;
        if report.totals.classes_with_lcom == 0 {
            return Ok(());
        }
        writeln!(w, "  most-split classes (cluster_count × method_count):")?;
        for class in report.worst_n(top_n) {
            writeln!(
                w,
                "    - {:>3} clusters / {:>3} methods  {}:{:<4}  {}",
                class.cluster_count,
                class.method_count,
                class.file.display(),
                class.start_line,
                class.class_name,
            )?;
        }
        Ok(())
    }

    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value {
        json!(ctx.reports.lcom.as_ref())
    }

    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value) {
        let n = ctx.cfg.metrics.top_n_lcom();
        let classes = ctx
            .reports
            .lcom
            .as_ref()
            .map(|r| r.worst_n(n))
            .unwrap_or_default();
        (n, json!({ "classes": classes }))
    }
}
