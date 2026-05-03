//! Per-metric section abstraction for `heal metrics`.
//!
//! Each metric (`loc`, `complexity`, `churn`, …) implements
//! [`MetricSection`] in its own file under `commands/metrics/`. The
//! orchestrator in [`super::run`] iterates the registry and calls the
//! trait methods — no metric-specific branching at the top level.
//!
//! `raw_json` / `worst_json` are split because the JSON shape under
//! `--metric X` substitutes a precomputed `worst` payload for the raw
//! report so skills don't have to sort + slice client-side; both
//! sections answer the same question at different verbosity levels.

use std::io::{self, Write};

use crate::cli::MetricKind;
use crate::core::config::Config;
use crate::core::term::{ansi_wrap, ANSI_CYAN};
use crate::observers::ObserverReports;

/// Read-only context handed to every section. Holding `cfg` and the
/// pre-computed `ObserverReports` here lets each section pull only the
/// slice it needs without re-running observers.
pub(super) struct SectionCtx<'a> {
    pub cfg: &'a Config,
    pub reports: &'a ObserverReports,
    pub colorize: bool,
}

/// Write the visual section divider every `MetricSection::render_text`
/// uses as its first line. Centralized so the divider rule (a blank
/// line followed by a cyan title bar) stays consistent and skips color
/// codes when stdout is not a TTY.
pub(super) fn write_section_header(
    label: &str,
    ctx: &SectionCtx<'_>,
    w: &mut dyn Write,
) -> io::Result<()> {
    writeln!(w)?;
    let title = format!("── {label} ──");
    writeln!(w, "{}", ansi_wrap(ANSI_CYAN, &title, ctx.colorize))?;
    Ok(())
}

pub(super) trait MetricSection {
    /// Metric tag. Drives both `--metric <kind>` filtering and the JSON
    /// `metric` echo field.
    fn metric(&self) -> MetricKind;

    /// Render the human-readable text summary. No-op when the section's
    /// observer ran with no signal (e.g. `churn` outside a git repo).
    fn render_text(&self, ctx: &SectionCtx<'_>, w: &mut dyn Write) -> io::Result<()>;

    /// Full report payload included in the unfiltered (`--json` without
    /// `--metric`) output. Returns `Value::Null` when the observer
    /// produced nothing — preserves the pre-refactor behavior where
    /// `r.churn.as_ref()` serialized to `null`.
    fn raw_json(&self, ctx: &SectionCtx<'_>) -> serde_json::Value;

    /// `(top_n, worst_payload)` for the `--json --metric <kind>` path.
    /// Empty payloads (no signal) still return the configured `top_n`
    /// so consumers can distinguish "ran with no findings" from "ran
    /// with a smaller window".
    fn worst_json(&self, ctx: &SectionCtx<'_>) -> (usize, serde_json::Value);
}

/// All sections in canonical ordering. The text renderer prints in this
/// order; the JSON consumer doesn't see ordering since maps are
/// unordered. Add new sections to the bottom.
pub(super) fn all_sections() -> Vec<Box<dyn MetricSection>> {
    vec![
        Box::new(super::loc::LocSection),
        Box::new(super::complexity::ComplexitySection),
        Box::new(super::churn::ChurnSection),
        Box::new(super::coupling::ChangeCouplingSection),
        Box::new(super::duplication::DuplicationSection),
        Box::new(super::hotspot::HotspotSection),
        Box::new(super::lcom::LcomSection),
    ]
}
