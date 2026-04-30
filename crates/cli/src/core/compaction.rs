//! Compaction policy for event-log directories.
//!
//! [`EventLog`] keeps month-rotated `YYYY-MM.jsonl` files in a single
//! directory. Without compaction, every commit / hook event lives
//! forever as plaintext. Compaction is two cheap passes:
//!
//!   - Segments older than `gzip_after` (90d default) are rewritten
//!     in place as `<dir>/YYYY-MM.jsonl.gz`. The original `.jsonl` is
//!     removed. `EventLog::segments` already prefers the gzipped
//!     version for the same month, so readers see no schema change.
//!   - Segments older than `delete_after` (365d default) are removed
//!     entirely. Beyond a year there is no realistic reader for the
//!     event log: snapshots are reconstructed from `git log`,
//!     calibration uses the last 90 days, and `heal cache log` /
//!     `heal logs` are operator views of recent activity.
//!
//! Both passes are idempotent — re-running on a state that's already
//! compacted is a no-op.

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, TimeZone, Utc};
use flate2::write::GzEncoder;
use flate2::Compression;

use crate::core::error::{Error, Result};
use crate::core::eventlog::{EventLog, Segment};
use crate::core::fs::atomic_write;
use crate::core::HealPaths;

/// Age thresholds for the two compaction passes. Defaults match the
/// v0.2 spec (90 / 365 days).
#[derive(Debug, Clone, Copy)]
pub struct CompactionPolicy {
    pub gzip_after: Duration,
    pub delete_after: Duration,
}

impl CompactionPolicy {
    pub const DEFAULT_GZIP_AFTER_DAYS: i64 = 90;
    pub const DEFAULT_DELETE_AFTER_DAYS: i64 = 365;
}

impl Default for CompactionPolicy {
    fn default() -> Self {
        Self {
            gzip_after: Duration::days(Self::DEFAULT_GZIP_AFTER_DAYS),
            delete_after: Duration::days(Self::DEFAULT_DELETE_AFTER_DAYS),
        }
    }
}

/// Per-run summary of work performed.
#[derive(Debug, Default, Clone)]
pub struct CompactionStats {
    /// Active-month segments compressed in place.
    pub gzipped: Vec<PathBuf>,
    /// Segments deleted because they were past `delete_after`.
    pub deleted: Vec<PathBuf>,
}

impl CompactionStats {
    #[must_use]
    pub fn touched(&self) -> bool {
        !self.gzipped.is_empty() || !self.deleted.is_empty()
    }
}

/// Run both passes against `log`. Idempotent — safe to call from
/// `heal hook commit` even when nothing is ripe yet.
pub fn compact(
    log: &EventLog,
    policy: &CompactionPolicy,
    now: DateTime<Utc>,
) -> Result<CompactionStats> {
    let mut stats = CompactionStats::default();
    for segment in log.segments()? {
        if is_older_than(&segment, policy.delete_after, now) {
            std::fs::remove_file(&segment.path).map_err(|e| Error::Io {
                path: segment.path.clone(),
                source: e,
            })?;
            stats.deleted.push(segment.path);
            continue;
        }
        if is_older_than(&segment, policy.gzip_after, now) && !segment.compressed {
            let gz_path = gzip_path(&segment.path);
            let bytes = std::fs::read(&segment.path).map_err(|e| Error::Io {
                path: segment.path.clone(),
                source: e,
            })?;
            atomic_write(&gz_path, &gzip(&bytes, &gz_path)?)?;
            std::fs::remove_file(&segment.path).map_err(|e| Error::Io {
                path: segment.path.clone(),
                source: e,
            })?;
            stats.gzipped.push(gz_path);
        }
    }
    Ok(stats)
}

/// Run [`compact`] across every event-log dir under `.heal/`. The
/// hook and the `heal compact` CLI both go through this so the
/// fan-out lives in `core::` rather than in `commands::` (where
/// `heal hook commit` shouldn't reach).
pub fn compact_all(
    paths: &HealPaths,
    policy: &CompactionPolicy,
    now: DateTime<Utc>,
) -> Result<Vec<(&'static str, CompactionStats)>> {
    [
        ("snapshots", paths.snapshots_dir()),
        ("logs", paths.logs_dir()),
        ("checks", paths.checks_dir()),
    ]
    .into_iter()
    .map(|(label, dir)| compact(&EventLog::new(&dir), policy, now).map(|stats| (label, stats)))
    .collect()
}

/// `<dir>/2025-12.jsonl` → `<dir>/2025-12.jsonl.gz`.
fn gzip_path(plaintext: &Path) -> PathBuf {
    let mut s = plaintext.as_os_str().to_owned();
    s.push(".gz");
    PathBuf::from(s)
}

/// Compress `bytes` with gzip. `path` is only used for error context.
fn gzip(bytes: &[u8], path: &Path) -> Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    encoder.finish().map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })
}

/// A segment is "older than X" once the first day of its *following*
/// month is more than X in the past — i.e. once the segment is
/// definitively complete. Conservative: never compacts a month while
/// it's still receiving writes.
fn is_older_than(segment: &Segment, threshold: Duration, now: DateTime<Utc>) -> bool {
    let completed = segment_completed_at(segment.year, segment.month);
    now - completed >= threshold
}

fn segment_completed_at(year: i32, month: u32) -> DateTime<Utc> {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    Utc.with_ymd_and_hms(ny, nm, 1, 0, 0, 0)
        .single()
        .expect("valid first-of-month")
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::core::eventlog::Event;
    use serde_json::json;
    use tempfile::tempdir;

    fn write_event(log: &EventLog, ts: DateTime<Utc>, payload: i64) {
        log.append(&Event {
            timestamp: ts,
            event: "test".into(),
            data: json!({ "n": payload }),
        })
        .expect("append");
    }

    #[test]
    fn gzips_old_segment_in_place() {
        // Segment age between gzip_after (90d) and delete_after (365d).
        let dir = tempdir().unwrap();
        let log = EventLog::new(dir.path());
        let old = Utc.with_ymd_and_hms(2025, 12, 15, 0, 0, 0).unwrap();
        write_event(&log, old, 1);
        write_event(&log, old + Duration::hours(1), 2);

        let now = Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap();
        let stats = compact(&log, &CompactionPolicy::default(), now).unwrap();

        assert_eq!(stats.gzipped.len(), 1);
        assert!(stats.deleted.is_empty());
        assert!(!dir.path().join("2025-12.jsonl").exists());
        assert!(dir.path().join("2025-12.jsonl.gz").exists());

        // Reader is unchanged: prefers `.gz` for the same month and
        // round-trips events back.
        let segments = log.segments().unwrap();
        assert_eq!(segments.len(), 1);
        assert!(segments[0].compressed);
        let events: Vec<_> = EventLog::iter_segments(segments)
            .filter_map(std::result::Result::ok)
            .collect();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn deletes_segments_past_year() {
        let dir = tempdir().unwrap();
        let log = EventLog::new(dir.path());
        let ancient = Utc.with_ymd_and_hms(2024, 1, 5, 0, 0, 0).unwrap();
        write_event(&log, ancient, 1);

        let now = Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap();
        let stats = compact(&log, &CompactionPolicy::default(), now).unwrap();

        assert_eq!(stats.deleted.len(), 1);
        assert!(!dir.path().join("2024-01.jsonl").exists());
        assert!(log.segments().unwrap().is_empty());
    }

    #[test]
    fn does_not_touch_recent_segments() {
        let dir = tempdir().unwrap();
        let log = EventLog::new(dir.path());
        let recent = Utc.with_ymd_and_hms(2026, 3, 10, 0, 0, 0).unwrap();
        write_event(&log, recent, 1);

        let now = Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap();
        let stats = compact(&log, &CompactionPolicy::default(), now).unwrap();

        assert!(!stats.touched());
        assert!(dir.path().join("2026-03.jsonl").exists());
    }

    #[test]
    fn second_pass_is_a_noop() {
        let dir = tempdir().unwrap();
        let log = EventLog::new(dir.path());
        let old = Utc.with_ymd_and_hms(2025, 12, 15, 0, 0, 0).unwrap();
        write_event(&log, old, 1);
        let now = Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap();
        compact(&log, &CompactionPolicy::default(), now).unwrap();
        let stats2 = compact(&log, &CompactionPolicy::default(), now).unwrap();
        assert!(!stats2.touched());
    }

    #[test]
    fn segment_completed_at_handles_year_boundary() {
        let dec = segment_completed_at(2025, 12);
        assert_eq!(dec, Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap());
        let jan = segment_completed_at(2025, 1);
        assert_eq!(jan, Utc.with_ymd_and_hms(2025, 2, 1, 0, 0, 0).unwrap());
    }

    #[test]
    fn missing_directory_is_a_noop() {
        let dir = tempdir().unwrap();
        let log = EventLog::new(dir.path().join("does-not-exist"));
        let now = Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap();
        let stats = compact(&log, &CompactionPolicy::default(), now).unwrap();
        assert!(!stats.touched());
    }
}
