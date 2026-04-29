use std::io::Write;

use chrono::{TimeZone, Utc};
use flate2::write::GzEncoder;
use heal_cli::core::eventlog::{Event, EventLog};
use serde_json::json;

#[test]
fn append_rotates_per_calendar_month() {
    let dir = tempfile::tempdir().unwrap();
    let log = EventLog::new(dir.path());

    let s1 = Event {
        timestamp: Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap(),
        event: "commit".into(),
        data: json!({"sha": "deadbeef"}),
    };
    let s2 = Event {
        timestamp: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
        event: "commit".into(),
        data: json!({"sha": "cafebabe"}),
    };
    log.append(&s1).unwrap();
    log.append(&s2).unwrap();

    let mar = dir.path().join("2026-03.jsonl");
    let apr = dir.path().join("2026-04.jsonl");
    assert!(mar.exists(), "march segment missing");
    assert!(apr.exists(), "april segment missing");
    assert_eq!(std::fs::read_to_string(&mar).unwrap().lines().count(), 1);
    assert_eq!(std::fs::read_to_string(&apr).unwrap().lines().count(), 1);
}

#[test]
fn appends_within_same_month_share_file() {
    let dir = tempfile::tempdir().unwrap();
    let log = EventLog::new(dir.path());
    for i in 0..5 {
        log.append(&Event {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 10 + i, 12, 0, 0).unwrap(),
            event: "edit".into(),
            data: json!({"i": i}),
        })
        .unwrap();
    }
    let lines = std::fs::read_to_string(dir.path().join("2026-04.jsonl"))
        .unwrap()
        .lines()
        .count();
    assert_eq!(lines, 5);
}

#[test]
fn reader_iterates_in_chronological_order() {
    let dir = tempfile::tempdir().unwrap();
    let log = EventLog::new(dir.path());
    let april = Event {
        timestamp: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
        event: "april".into(),
        data: json!(null),
    };
    let february = Event {
        timestamp: Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap(),
        event: "february".into(),
        data: json!(null),
    };
    let march = Event {
        timestamp: Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap(),
        event: "march".into(),
        data: json!(null),
    };
    log.append(&april).unwrap();
    log.append(&february).unwrap();
    log.append(&march).unwrap();

    let events: Vec<_> = log.try_iter().unwrap().map(|r| r.unwrap().event).collect();
    assert_eq!(events, vec!["february", "march", "april"]);
}

#[test]
fn reader_handles_gzipped_segments() {
    let dir = tempfile::tempdir().unwrap();
    // Write a fake compressed past month directly.
    let path = dir.path().join("2026-01.jsonl.gz");
    let event = Event {
        timestamp: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        event: "compressed".into(),
        data: json!({"k": "v"}),
    };
    let line = serde_json::to_string(&event).unwrap() + "\n";
    let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(line.as_bytes()).unwrap();
    std::fs::write(&path, enc.finish().unwrap()).unwrap();

    let log = EventLog::new(dir.path());
    let events: Vec<_> = log.try_iter().unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "compressed");
}

#[test]
fn reader_prefers_gzipped_over_plaintext_for_same_month() {
    let dir = tempfile::tempdir().unwrap();
    // Plaintext (stale, should be ignored once .gz exists).
    let plain = Event {
        timestamp: Utc.with_ymd_and_hms(2026, 1, 5, 0, 0, 0).unwrap(),
        event: "plain".into(),
        data: json!(null),
    };
    std::fs::write(
        dir.path().join("2026-01.jsonl"),
        serde_json::to_string(&plain).unwrap() + "\n",
    )
    .unwrap();
    // Compressed (canonical).
    let canonical = Event {
        timestamp: Utc.with_ymd_and_hms(2026, 1, 6, 0, 0, 0).unwrap(),
        event: "canonical".into(),
        data: json!(null),
    };
    let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all((serde_json::to_string(&canonical).unwrap() + "\n").as_bytes())
        .unwrap();
    std::fs::write(dir.path().join("2026-01.jsonl.gz"), enc.finish().unwrap()).unwrap();

    let log = EventLog::new(dir.path());
    let events: Vec<_> = log.try_iter().unwrap().map(|r| r.unwrap().event).collect();
    assert_eq!(events, vec!["canonical"]);
}

#[test]
fn reader_skips_unrelated_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "not a segment").unwrap();
    std::fs::write(dir.path().join("2026-13.jsonl"), "invalid month").unwrap();
    let log = EventLog::new(dir.path());
    assert_eq!(log.segments().unwrap().len(), 0);
}
