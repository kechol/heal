//! Append-only event log with month-based rotation.
//!
//! Used by both `.heal/snapshots/` (commit-time `MetricsSnapshot` events) and
//! `.heal/logs/` (Claude / git hook raw events). Each call site picks its
//! directory and reuses the same writer/reader machinery.
//!
//! Layout under `<dir>/`:
//!   2026-04.jsonl       # current month, plaintext, append-only
//!   2026-03.jsonl       # past month, plaintext (compression deferred to v0.2+)
//!   2026-01.jsonl.gz    # compressed past month (forward-compat reader)
//!
//! v0.1 only writes plaintext; reading already handles `.gz` so future
//! compaction passes can land without touching call sites.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

use chrono::{DateTime, Datelike, Utc};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// One row in `<dir>/*.jsonl`.
///
/// `data` is opaque JSON so each event source (commit hook, edit hook, Claude
/// stop hook, scan) can attach its own payload without bloating the core
/// schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Event {
    pub timestamp: DateTime<Utc>,
    pub event: String,
    pub data: serde_json::Value,
}

impl Event {
    pub fn new(event: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            timestamp: Utc::now(),
            event: event.into(),
            data,
        }
    }
}

/// Directory-scoped append + read API. Both `EventLog::append` and
/// `EventLog::segments` operate on the same `<dir>/YYYY-MM.jsonl` layout.
/// Cheap to construct; the writer reopens the target file per append so
/// cross-process appends don't collide on a stale handle.
#[derive(Debug, Clone)]
pub struct EventLog {
    dir: PathBuf,
}

impl EventLog {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    #[must_use]
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    /// Append one event to the current-month file, creating the directory if
    /// needed.
    pub fn append(&self, event: &Event) -> Result<()> {
        std::fs::create_dir_all(&self.dir).map_err(|e| Error::Io {
            path: self.dir.clone(),
            source: e,
        })?;

        let path = self.path_for(&event.timestamp);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| Error::Io {
                path: path.clone(),
                source: e,
            })?;

        let mut w = BufWriter::new(file);
        let line = serde_json::to_string(event).expect("Event serialization is infallible");
        writeln!(w, "{line}").map_err(|e| Error::Io {
            path: path.clone(),
            source: e,
        })?;
        w.flush().map_err(|e| Error::Io { path, source: e })?;
        Ok(())
    }

    /// Enumerate month files in (year, month) order. Returns an empty vec if
    /// the directory is absent.
    pub fn segments(&self) -> Result<Vec<Segment>> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(it) => it,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(Error::Io {
                    path: self.dir.clone(),
                    source: e,
                })
            }
        };
        // BTreeMap keyed on (year, month) keeps iteration deterministic and
        // tolerates duplicate `.jsonl` + `.jsonl.gz` for the same month
        // (compressed wins, since the writer should have removed plaintext).
        let mut by_month: BTreeMap<(i32, u32), Segment> = BTreeMap::new();
        for entry in entries {
            let entry = entry.map_err(|e| Error::Io {
                path: self.dir.clone(),
                source: e,
            })?;
            let Some(segment) = Segment::from_path(entry.path()) else {
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

    /// Iterate every event across every segment in chronological order.
    /// Construction is fallible because we glob the segment directory up
    /// front; per-record errors surface inside the iterator.
    pub fn try_iter(&self) -> Result<impl Iterator<Item = Result<Event>>> {
        Ok(Self::iter_segments(self.segments()?))
    }

    /// Iterate over a pre-computed segment list. Useful when the caller
    /// already paid for `segments()` and wants to avoid re-globbing.
    pub fn iter_segments(segments: Vec<Segment>) -> impl Iterator<Item = Result<Event>> {
        SegmentIter::new(segments)
    }

    fn path_for(&self, ts: &DateTime<Utc>) -> PathBuf {
        self.dir
            .join(format!("{:04}-{:02}.jsonl", ts.year(), ts.month()))
    }
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub year: i32,
    pub month: u32,
    pub compressed: bool,
    pub path: PathBuf,
}

impl Segment {
    /// Parse a `.jsonl` / `.jsonl.gz` segment filename into a `Segment`.
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
    segments: std::vec::IntoIter<Segment>,
    current: Option<(PathBuf, std::io::Lines<Box<dyn BufRead>>)>,
}

impl SegmentIter {
    fn new(segments: Vec<Segment>) -> Self {
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
    type Item = Result<Event>;

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
                        Error::EventLogParse {
                            path: path.clone(),
                            source,
                        }
                    }));
                }
            }
        }
    }
}
