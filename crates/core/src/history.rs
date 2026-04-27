//! Append-only history log with month-based rotation.
//!
//! Layout:
//!   .heal/history/
//!     2026-04.jsonl       # current month, plaintext, append-only
//!     2026-03.jsonl       # past month, plaintext (compression deferred to v0.2+)
//!     2026-01.jsonl.gz    # compressed past month (forward-compat reader)
//!
//! v0.1 only writes plaintext; reading already handles `.gz` so future
//! compaction passes can land without touching call sites.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;

use chrono::{DateTime, Datelike, Utc};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// One row in `history/*.jsonl`.
///
/// `data` is opaque JSON so each event source (commit hook, edit hook, scan)
/// can attach its own payload without bloating the core schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Snapshot {
    pub timestamp: DateTime<Utc>,
    pub event: String,
    pub data: serde_json::Value,
}

impl Snapshot {
    pub fn new(event: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            timestamp: Utc::now(),
            event: event.into(),
            data,
        }
    }
}

/// Current `MetricsSnapshot::version`. Bump on breaking field renames so the
/// reader can skip records it can't decode rather than failing the whole iter.
pub const METRICS_SNAPSHOT_VERSION: u32 = 1;

/// Per-commit roll-up of every observer's report, plus an optional delta
/// against the prior snapshot. Persisted as the `data` payload of a
/// `Snapshot` whose `event` is `"commit"`.
///
/// The per-metric reports are held as opaque `serde_json::Value` because the
/// concrete report types live in `heal-observer`, and `heal-core` cannot
/// depend on it without a workspace cycle. Callers in `heal-cli` serialize
/// each typed report at construction time and deserialize as needed.
///
/// Forward-compat note: this struct deliberately does **not** use
/// `deny_unknown_fields`. Older binaries reading newer history files should
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

/// Append-only writer. Cheap to construct; reopens the target file per append
/// so cross-process appends don't collide on a stale handle.
#[derive(Debug, Clone)]
pub struct HistoryWriter {
    history_dir: PathBuf,
}

impl HistoryWriter {
    pub fn new(history_dir: impl Into<PathBuf>) -> Self {
        Self {
            history_dir: history_dir.into(),
        }
    }

    pub fn append(&self, snapshot: &Snapshot) -> Result<()> {
        std::fs::create_dir_all(&self.history_dir).map_err(|e| Error::Io {
            path: self.history_dir.clone(),
            source: e,
        })?;

        let path = self.path_for(&snapshot.timestamp);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| Error::Io {
                path: path.clone(),
                source: e,
            })?;

        let mut w = BufWriter::new(file);
        let line = serde_json::to_string(snapshot).expect("Snapshot serialization is infallible");
        writeln!(w, "{line}").map_err(|e| Error::Io {
            path: path.clone(),
            source: e,
        })?;
        w.flush().map_err(|e| Error::Io { path, source: e })?;
        Ok(())
    }

    fn path_for(&self, ts: &DateTime<Utc>) -> PathBuf {
        self.history_dir
            .join(format!("{:04}-{:02}.jsonl", ts.year(), ts.month()))
    }
}

/// Reader that walks all month files in chronological order, transparently
/// handling `.jsonl` and `.jsonl.gz` so callers don't care about compaction.
#[derive(Debug, Clone)]
pub struct HistoryReader {
    history_dir: PathBuf,
}

impl HistoryReader {
    pub fn new(history_dir: impl Into<PathBuf>) -> Self {
        Self {
            history_dir: history_dir.into(),
        }
    }

    /// Enumerate month files in (year, month) order. Returns an empty vec if
    /// the history directory is absent.
    pub fn segments(&self) -> Result<Vec<HistorySegment>> {
        let entries = match std::fs::read_dir(&self.history_dir) {
            Ok(it) => it,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(Error::Io {
                    path: self.history_dir.clone(),
                    source: e,
                })
            }
        };
        // BTreeMap keyed on (year, month) keeps iteration deterministic and
        // tolerates duplicate `.jsonl` + `.jsonl.gz` for the same month
        // (compressed wins, since the writer should have removed plaintext).
        let mut by_month: BTreeMap<(i32, u32), HistorySegment> = BTreeMap::new();
        for entry in entries {
            let entry = entry.map_err(|e| Error::Io {
                path: self.history_dir.clone(),
                source: e,
            })?;
            let Some(segment) = HistorySegment::from_path(entry.path()) else {
                continue;
            };
            by_month
                .entry((segment.year, segment.month))
                .and_modify(|prev| {
                    if segment.compressed && !prev.compressed {
                        *prev = segment.clone();
                    }
                })
                .or_insert(segment);
        }
        Ok(by_month.into_values().collect())
    }

    /// Iterate every snapshot across every segment in chronological order.
    ///
    /// Construction is fallible because we glob the segment directory up
    /// front; per-record errors surface inside the iterator.
    pub fn try_iter(&self) -> Result<impl Iterator<Item = Result<Snapshot>>> {
        Ok(Self::iter_segments(self.segments()?))
    }

    /// Iterate over a pre-computed segment list. Useful when the caller
    /// already paid for `segments()` (e.g. `heal status`) and wants to avoid
    /// re-globbing the directory.
    pub fn iter_segments(segments: Vec<HistorySegment>) -> impl Iterator<Item = Result<Snapshot>> {
        SegmentIter::new(segments)
    }

    /// Walk segments in **reverse chronological order** and return the most
    /// recent snapshot whose `data` decodes as a `MetricsSnapshot`. Records
    /// that fail the decode (legacy commit payloads, hook events with a
    /// different shape) are skipped silently. Returns `Ok(None)` for an
    /// empty / nonexistent history directory.
    ///
    /// The single-segment in-memory load is acceptable for v0.1 month sizes;
    /// if month files ever exceed double-digit MB we'd want a real reverse-
    /// line iterator over `BufRead`.
    pub fn latest_metrics_snapshot(&self) -> Result<Option<(Snapshot, MetricsSnapshot)>> {
        Self::latest_metrics_snapshot_from(self.segments()?)
    }

    /// Same as [`Self::latest_metrics_snapshot`] over a pre-globbed segment
    /// list. Useful when the caller (e.g. `heal status`) already paid for
    /// `segments()` and wants to avoid re-scanning the directory.
    pub fn latest_metrics_snapshot_from(
        mut segments: Vec<HistorySegment>,
    ) -> Result<Option<(Snapshot, MetricsSnapshot)>> {
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
                let snap: Snapshot =
                    serde_json::from_str(line).map_err(|source| Error::HistoryParse {
                        path: seg.path.clone(),
                        source,
                    })?;
                if let Ok(metrics) = serde_json::from_value::<MetricsSnapshot>(snap.data.clone()) {
                    return Ok(Some((snap, metrics)));
                }
            }
        }
        Ok(None)
    }
}

#[derive(Debug, Clone)]
pub struct HistorySegment {
    pub year: i32,
    pub month: u32,
    pub compressed: bool,
    pub path: PathBuf,
}

impl HistorySegment {
    /// Parse a `.jsonl` / `.jsonl.gz` segment filename into a `HistorySegment`.
    /// Returns `None` for unrelated files or out-of-range months.
    fn from_path(path: PathBuf) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        let (stem, compressed) = if let Some(stripped) = name.strip_suffix(".jsonl.gz") {
            (stripped, true)
        } else if let Some(stripped) = name.strip_suffix(".jsonl") {
            (stripped, false)
        } else {
            return None;
        };
        let (y, m) = stem.split_once('-')?;
        let year: i32 = y.parse().ok()?;
        let month: u32 = m.parse().ok()?;
        if !(1..=12).contains(&month) {
            return None;
        }
        Some(Self {
            year,
            month,
            compressed,
            path,
        })
    }

    pub fn open(&self) -> Result<Box<dyn BufRead>> {
        let file = File::open(&self.path).map_err(|e| Error::Io {
            path: self.path.clone(),
            source: e,
        })?;
        if self.compressed {
            Ok(Box::new(BufReader::new(GzDecoder::new(file))))
        } else {
            Ok(Box::new(BufReader::new(file)))
        }
    }
}

struct SegmentIter {
    segments: std::vec::IntoIter<HistorySegment>,
    current: Option<(PathBuf, std::io::Lines<Box<dyn BufRead>>)>,
}

impl SegmentIter {
    fn new(segments: Vec<HistorySegment>) -> Self {
        Self {
            segments: segments.into_iter(),
            current: None,
        }
    }

    fn advance(&mut self) -> Result<bool> {
        let Some(seg) = self.segments.next() else {
            self.current = None;
            return Ok(false);
        };
        let reader = seg.open()?;
        self.current = Some((seg.path, reader.lines()));
        Ok(true)
    }
}

impl Iterator for SegmentIter {
    type Item = Result<Snapshot>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.current.is_none() {
                match self.advance() {
                    Ok(true) => {}
                    Ok(false) => return None,
                    Err(e) => return Some(Err(e)),
                }
            }
            let (path, lines) = self.current.as_mut().expect("just primed");
            match lines.next() {
                None => {
                    self.current = None;
                }
                Some(Err(e)) => {
                    return Some(Err(Error::Io {
                        path: path.clone(),
                        source: e,
                    }));
                }
                Some(Ok(line)) if line.trim().is_empty() => {}
                Some(Ok(line)) => {
                    return Some(serde_json::from_str(&line).map_err(|source| {
                        Error::HistoryParse {
                            path: path.clone(),
                            source,
                        }
                    }));
                }
            }
        }
    }
}
