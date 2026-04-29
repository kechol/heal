//! `MetricsSnapshot` and per-metric delta types persisted in `.heal/snapshots/`.
//!
//! These are the typed payloads that ride inside `Event::data` for the
//! `commit` event. The generic event-log machinery (rotation, append, read)
//! lives in [`crate::eventlog`]; this module owns only the metric-shaped
//! types and the helper for finding the most recent metrics record.

use std::io::Read;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::eventlog::{Event, EventLog, Segment};

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
            delta: None,
        }
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
        Self::latest_in_segments(log.segments()?)
    }

    /// Same as [`Self::latest_in`] over a pre-globbed segment list. Useful
    /// when the caller (e.g. `heal status`) already paid for `segments()` and
    /// wants to avoid re-scanning the directory.
    pub fn latest_in_segments(mut segments: Vec<Segment>) -> Result<Option<(Event, Self)>> {
        segments.reverse();
        for seg in segments {
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
