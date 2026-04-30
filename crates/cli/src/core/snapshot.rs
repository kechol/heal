//! `MetricsSnapshot` and per-metric delta types persisted in `.heal/snapshots/`.
//!
//! These are the typed payloads that ride inside `Event::data` for the
//! `commit` event. The generic event-log machinery (rotation, append, read)
//! lives in [`crate::core::eventlog`]; this module owns only the metric-shaped
//! types and the helper for finding the most recent metrics record.

use std::io::Read;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::error::{Error, Result};
use crate::core::eventlog::{Event, EventLog, Segment};
use crate::core::severity::Severity;

/// Current `MetricsSnapshot::version`. Bump on breaking field renames so the
/// reader can skip records it can't decode rather than failing the whole iter.
pub const METRICS_SNAPSHOT_VERSION: u32 = 1;

/// Per-commit roll-up of every observer's report, plus an optional delta
/// against the prior snapshot. Persisted as the `data` payload of an
/// [`Event`] whose `event` is `"commit"`.
///
/// The per-metric reports are held as opaque `serde_json::Value` because the
/// concrete report types live in `heal-observer`, and `heal-core` cannot
/// depend on it without a workspace cycle. Callers in `heal-cli` serialize
/// each typed report at construction time and deserialize as needed.
///
/// Forward-compat note: this struct deliberately does **not** use
/// `deny_unknown_fields`. Older binaries reading newer snapshot files should
/// silently ignore additions; readers that hit a `version` higher than they
/// know can fall back to skipping the record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsSnapshot {
    pub version: u32,
    #[serde(default)]
    pub git_sha: Option<String>,
    #[serde(default)]
    pub loc: Option<serde_json::Value>,
    #[serde(default)]
    pub complexity: Option<serde_json::Value>,
    #[serde(default)]
    pub churn: Option<serde_json::Value>,
    #[serde(default)]
    pub change_coupling: Option<serde_json::Value>,
    #[serde(default)]
    pub duplication: Option<serde_json::Value>,
    #[serde(default)]
    pub hotspot: Option<serde_json::Value>,
    #[serde(default)]
    pub lcom: Option<serde_json::Value>,
    /// Severity tally produced by Calibration. Older snapshots predate
    /// this field — `None` means "the writer hadn't classified yet"
    /// (legacy, or the project is missing `.heal/calibration.toml`),
    /// not "everything was Ok". Display layers should distinguish.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity_counts: Option<SeverityCounts>,
    /// Codebase size at snapshot time, used by the `heal calibrate
    /// --check` file-count drift trigger. `None` on legacy records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codebase_files: Option<u32>,
    /// Filled in by the snapshot writer when a prior snapshot is available.
    #[serde(default)]
    pub delta: Option<serde_json::Value>,
}

impl Default for MetricsSnapshot {
    fn default() -> Self {
        Self {
            version: METRICS_SNAPSHOT_VERSION,
            git_sha: None,
            loc: None,
            complexity: None,
            churn: None,
            change_coupling: None,
            duplication: None,
            hotspot: None,
            lcom: None,
            severity_counts: None,
            codebase_files: None,
            delta: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeverityCounts {
    #[serde(default)]
    pub critical: u32,
    #[serde(default)]
    pub high: u32,
    #[serde(default)]
    pub medium: u32,
    #[serde(default)]
    pub ok: u32,
}

impl SeverityCounts {
    /// Tally one classification result. Saturating-add so a 4-billion
    /// finding codebase doesn't wrap to 0 (it would have other
    /// problems by then).
    pub fn tally(&mut self, severity: Severity) {
        let bucket = match severity {
            Severity::Critical => &mut self.critical,
            Severity::High => &mut self.high,
            Severity::Medium => &mut self.medium,
            Severity::Ok => &mut self.ok,
        };
        *bucket = bucket.saturating_add(1);
    }

    /// Inline summary line for human-facing CLI output, e.g.
    /// `[critical] 3   [high] 12   [medium] 28   [ok] 412`. When
    /// `colorize` is true the four labels carry ANSI SGR codes (red /
    /// yellow / cyan / green) suitable for a terminal; pass `false`
    /// when piping to a file.
    #[must_use]
    pub fn render_inline(&self, colorize: bool) -> String {
        format!(
            "{} {}   {} {}   {} {}   {} {}",
            ansi_wrap(ANSI_RED, "[critical]", colorize),
            self.critical,
            ansi_wrap(ANSI_YELLOW, "[high]", colorize),
            self.high,
            ansi_wrap(ANSI_CYAN, "[medium]", colorize),
            self.medium,
            ansi_wrap(ANSI_GREEN, "[ok]", colorize),
            self.ok,
        )
    }
}

/// ANSI SGR colour codes. `pub(crate)` so the small set of CLI commands
/// that emit colour can share the constants instead of redefining them.
pub(crate) const ANSI_RED: &str = "\x1b[31m";
pub(crate) const ANSI_GREEN: &str = "\x1b[32m";
pub(crate) const ANSI_YELLOW: &str = "\x1b[33m";
pub(crate) const ANSI_CYAN: &str = "\x1b[36m";

/// Wrap `text` in `color` (one of `ANSI_*`) followed by the SGR reset
/// when `enabled`; otherwise return `text` unchanged. Centralises the
/// `is_terminal()` gating that every colorising call site does.
#[must_use]
pub(crate) fn ansi_wrap(color: &str, text: &str, enabled: bool) -> String {
    if enabled {
        format!("{color}{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}

impl MetricsSnapshot {
    /// Walk segments in **reverse chronological order** and return the most
    /// recent event whose `data` decodes as a `MetricsSnapshot`. Records that
    /// fail the decode (legacy payloads, hook events with a different shape)
    /// are skipped silently. Returns `Ok(None)` for an empty / nonexistent
    /// directory.
    ///
    /// The single-segment in-memory load is acceptable for v0.1 month sizes;
    /// if month files ever exceed double-digit MB we'd want a real reverse-
    /// line iterator over `BufRead`.
    pub fn latest_in(log: &EventLog) -> Result<Option<(Event, Self)>> {
        Self::latest_in_segments(&log.segments()?)
    }

    /// Same as [`Self::latest_in`] over a pre-globbed segment list. Useful
    /// when the caller (e.g. `heal status`) already paid for `segments()` and
    /// wants to avoid re-scanning the directory.
    pub fn latest_in_segments(segments: &[Segment]) -> Result<Option<(Event, Self)>> {
        for seg in segments.iter().rev() {
            let mut buf = String::new();
            seg.open()?
                .read_to_string(&mut buf)
                .map_err(|e| Error::Io {
                    path: seg.path.clone(),
                    source: e,
                })?;
            for line in buf.lines().rev() {
                if line.trim().is_empty() {
                    continue;
                }
                // Skip records that fail to parse (legacy payloads, mid-write
                // truncation after SIGINT, future schema variants). The
                // module doc-contract is "skip silently" — propagating here
                // would brick `heal status` after a single corrupt line.
                let Ok(event) = serde_json::from_str::<Event>(line) else {
                    continue;
                };
                if let Ok(metrics) = serde_json::from_value::<Self>(event.data.clone()) {
                    return Ok(Some((event, metrics)));
                }
            }
        }
        Ok(None)
    }
}

/// Movement of every metric since the previous `MetricsSnapshot`. Persisted
/// inside the new snapshot's `delta` field so a `--since` query doesn't need
/// to recompute deltas record-by-record.
///
/// Per-metric entries are `Some(...)` only when both the previous and current
/// snapshots carried that metric. A user disabling a metric between commits
/// produces `None`, distinguishing "no movement" (zero delta) from "the
/// metric isn't comparable here".
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SnapshotDelta {
    pub from_sha: Option<String>,
    pub from_timestamp: Option<DateTime<Utc>>,
    pub complexity: Option<ComplexityDelta>,
    pub churn: Option<ChurnDelta>,
    pub hotspot: Option<HotspotDelta>,
    pub duplication: Option<DuplicationDelta>,
    pub change_coupling: Option<ChangeCouplingDelta>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComplexityDelta {
    pub max_ccn: i64,
    pub max_cognitive: i64,
    pub functions: i64,
    pub files: i64,
    /// Function display names that entered the top-N CCN ranking but were
    /// absent from the previous snapshot's top-N.
    pub new_top_ccn: Vec<String>,
    pub new_top_cognitive: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChurnDelta {
    pub commits_in_window: i64,
    pub top_file_changed: bool,
    pub previous_top_file: Option<String>,
    pub current_top_file: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HotspotDelta {
    pub max_score: f64,
    pub top_files_added: Vec<String>,
    pub top_files_dropped: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DuplicationDelta {
    pub duplicate_blocks: i64,
    pub duplicate_tokens: i64,
    pub files_affected: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeCouplingDelta {
    pub pairs: i64,
    pub files: i64,
    pub max_pair_count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_counts_render_inline_plain_has_no_ansi() {
        let c = SeverityCounts {
            critical: 3,
            high: 12,
            medium: 28,
            ok: 412,
        };
        let s = c.render_inline(false);
        assert!(
            !s.contains('\x1b'),
            "plain render must not include ANSI codes"
        );
        assert!(s.contains("[critical] 3"));
        assert!(s.contains("[high] 12"));
        assert!(s.contains("[medium] 28"));
        assert!(s.contains("[ok] 412"));
    }

    #[test]
    fn severity_counts_render_inline_colored_has_reset_after_each_label() {
        let c = SeverityCounts::default();
        let s = c.render_inline(true);
        // One SGR open + reset per label = 4 resets total.
        assert_eq!(s.matches("\x1b[0m").count(), 4);
    }
}
