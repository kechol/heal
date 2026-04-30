//! End-to-end coverage for `core::compaction`: round-trip a gzipped
//! segment through the reader and verify >365d data is dropped.

use chrono::{Duration, TimeZone, Utc};
use heal_cli::core::compaction::{compact, CompactionPolicy};
use heal_cli::core::eventlog::{Event, EventLog};
use serde_json::json;
use tempfile::tempdir;

fn ev(ts: chrono::DateTime<Utc>, n: i64) -> Event {
    Event {
        timestamp: ts,
        event: "test".into(),
        data: json!({ "n": n }),
    }
}

#[test]
fn gzipped_segment_is_readable_through_segments() {
    let dir = tempdir().unwrap();
    let log = EventLog::new(dir.path());
    let old = Utc.with_ymd_and_hms(2025, 12, 5, 0, 0, 0).unwrap();
    let recent = Utc.with_ymd_and_hms(2026, 4, 5, 0, 0, 0).unwrap();
    log.append(&ev(old, 1)).unwrap();
    log.append(&ev(old + Duration::days(2), 2)).unwrap();
    log.append(&ev(recent, 3)).unwrap();

    let now = Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap();
    compact(&log, &CompactionPolicy::default(), now).unwrap();

    let segments: Vec<_> = log
        .segments()
        .unwrap()
        .into_iter()
        .map(|s| (s.year, s.month, s.compressed))
        .collect();
    assert_eq!(segments, vec![(2025, 12, true), (2026, 4, false)]);

    // Reader transparently joins both, in chronological order.
    let events: Vec<_> = log
        .try_iter()
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.data["n"].as_i64().unwrap())
        .collect();
    assert_eq!(events, vec![1, 2, 3]);
}

#[test]
fn drops_segments_older_than_a_year() {
    let dir = tempdir().unwrap();
    let log = EventLog::new(dir.path());
    let ancient = Utc.with_ymd_and_hms(2024, 1, 5, 0, 0, 0).unwrap();
    let mid = Utc.with_ymd_and_hms(2025, 12, 5, 0, 0, 0).unwrap();
    let recent = Utc.with_ymd_and_hms(2026, 4, 5, 0, 0, 0).unwrap();
    log.append(&ev(ancient, 1)).unwrap();
    log.append(&ev(mid, 2)).unwrap();
    log.append(&ev(recent, 3)).unwrap();

    let now = Utc.with_ymd_and_hms(2026, 4, 30, 0, 0, 0).unwrap();
    let stats = compact(&log, &CompactionPolicy::default(), now).unwrap();
    assert_eq!(stats.gzipped.len(), 1);
    assert_eq!(stats.deleted.len(), 1);

    let events: Vec<_> = log
        .try_iter()
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.data["n"].as_i64().unwrap())
        .collect();
    // The 2024 entry is gone; 2025-12 (now gzipped) and 2026-04 remain.
    assert_eq!(events, vec![2, 3]);
}
