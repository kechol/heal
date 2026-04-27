use std::io::Write;

use chrono::{Datelike, TimeZone, Utc};
use flate2::write::GzEncoder;
use heal_core::history::{
    HistoryReader, HistoryWriter, MetricsSnapshot, Snapshot, METRICS_SNAPSHOT_VERSION,
};
use serde_json::json;

#[test]
fn append_rotates_per_calendar_month() {
    let dir = tempfile::tempdir().unwrap();
    let writer = HistoryWriter::new(dir.path());

    let s1 = Snapshot {
        timestamp: Utc.with_ymd_and_hms(2026, 3, 15, 10, 0, 0).unwrap(),
        event: "commit".into(),
        data: json!({"sha": "deadbeef"}),
    };
    let s2 = Snapshot {
        timestamp: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
        event: "commit".into(),
        data: json!({"sha": "cafebabe"}),
    };
    writer.append(&s1).unwrap();
    writer.append(&s2).unwrap();

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
    let writer = HistoryWriter::new(dir.path());
    for i in 0..5 {
        writer
            .append(&Snapshot {
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
    let writer = HistoryWriter::new(dir.path());
    let april = Snapshot {
        timestamp: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
        event: "april".into(),
        data: json!(null),
    };
    let february = Snapshot {
        timestamp: Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap(),
        event: "february".into(),
        data: json!(null),
    };
    let march = Snapshot {
        timestamp: Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap(),
        event: "march".into(),
        data: json!(null),
    };
    writer.append(&april).unwrap();
    writer.append(&february).unwrap();
    writer.append(&march).unwrap();

    let reader = HistoryReader::new(dir.path());
    let events: Vec<_> = reader
        .try_iter()
        .unwrap()
        .map(|r| r.unwrap().event)
        .collect();
    assert_eq!(events, vec!["february", "march", "april"]);
}

#[test]
fn reader_handles_gzipped_segments() {
    let dir = tempfile::tempdir().unwrap();
    // Write a fake compressed past month directly.
    let path = dir.path().join("2026-01.jsonl.gz");
    let snapshot = Snapshot {
        timestamp: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        event: "compressed".into(),
        data: json!({"k": "v"}),
    };
    let line = serde_json::to_string(&snapshot).unwrap() + "\n";
    let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(line.as_bytes()).unwrap();
    std::fs::write(&path, enc.finish().unwrap()).unwrap();

    let reader = HistoryReader::new(dir.path());
    let snapshots: Vec<_> = reader
        .try_iter()
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].event, "compressed");
}

#[test]
fn reader_prefers_gzipped_over_plaintext_for_same_month() {
    let dir = tempfile::tempdir().unwrap();
    // Plaintext (stale, should be ignored once .gz exists).
    let plain = Snapshot {
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
    let canonical = Snapshot {
        timestamp: Utc.with_ymd_and_hms(2026, 1, 6, 0, 0, 0).unwrap(),
        event: "canonical".into(),
        data: json!(null),
    };
    let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all((serde_json::to_string(&canonical).unwrap() + "\n").as_bytes())
        .unwrap();
    std::fs::write(dir.path().join("2026-01.jsonl.gz"), enc.finish().unwrap()).unwrap();

    let reader = HistoryReader::new(dir.path());
    let events: Vec<_> = reader
        .try_iter()
        .unwrap()
        .map(|r| r.unwrap().event)
        .collect();
    assert_eq!(events, vec!["canonical"]);
}

#[test]
fn metrics_snapshot_round_trips_through_writer() {
    let dir = tempfile::tempdir().unwrap();
    let writer = HistoryWriter::new(dir.path());

    let metrics = MetricsSnapshot {
        version: METRICS_SNAPSHOT_VERSION,
        git_sha: Some("a3b1c2f".into()),
        loc: Some(json!({"primary": "rust"})),
        complexity: Some(json!({"max_ccn": 12})),
        churn: None,
        change_coupling: None,
        duplication: None,
        hotspot: None,
        delta: None,
    };
    let snapshot = Snapshot {
        timestamp: Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap(),
        event: "commit".into(),
        data: serde_json::to_value(&metrics).unwrap(),
    };
    writer.append(&snapshot).unwrap();

    let reader = HistoryReader::new(dir.path());
    let snapshots: Vec<_> = reader
        .try_iter()
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(snapshots.len(), 1);
    let decoded: MetricsSnapshot = serde_json::from_value(snapshots[0].data.clone()).unwrap();
    assert_eq!(decoded, metrics);
}

#[test]
fn latest_metrics_snapshot_returns_most_recent() {
    let dir = tempfile::tempdir().unwrap();
    let writer = HistoryWriter::new(dir.path());
    let mk = |month: u32, sha: &str| -> Snapshot {
        let m = MetricsSnapshot {
            git_sha: Some(sha.into()),
            ..MetricsSnapshot::default()
        };
        Snapshot {
            timestamp: Utc.with_ymd_and_hms(2026, month, 1, 0, 0, 0).unwrap(),
            event: "commit".into(),
            data: serde_json::to_value(&m).unwrap(),
        }
    };
    writer.append(&mk(2, "feb")).unwrap();
    writer.append(&mk(3, "mar")).unwrap();
    writer.append(&mk(4, "apr")).unwrap();

    let reader = HistoryReader::new(dir.path());
    let (snap, metrics) = reader.latest_metrics_snapshot().unwrap().unwrap();
    assert_eq!(snap.timestamp.month(), 4);
    assert_eq!(metrics.git_sha.as_deref(), Some("apr"));
}

#[test]
fn latest_metrics_snapshot_skips_legacy_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let writer = HistoryWriter::new(dir.path());

    let real = MetricsSnapshot {
        git_sha: Some("real".into()),
        ..MetricsSnapshot::default()
    };
    writer
        .append(&Snapshot {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
            event: "commit".into(),
            data: serde_json::to_value(&real).unwrap(),
        })
        .unwrap();
    // Legacy payload from a pre-MetricsSnapshot binary, written *after* the
    // real snapshot — must be skipped so the real one is returned.
    writer
        .append(&Snapshot {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 2, 0, 0, 0).unwrap(),
            event: "edit".into(),
            data: json!("raw stdin string"),
        })
        .unwrap();

    let reader = HistoryReader::new(dir.path());
    let (_, metrics) = reader.latest_metrics_snapshot().unwrap().unwrap();
    assert_eq!(metrics.git_sha.as_deref(), Some("real"));
}

#[test]
fn latest_metrics_snapshot_empty_dir_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let reader = HistoryReader::new(dir.path().join("missing"));
    assert!(reader.latest_metrics_snapshot().unwrap().is_none());
}

#[test]
fn metrics_snapshot_default_version_is_current() {
    let snap = MetricsSnapshot::default();
    assert_eq!(snap.version, METRICS_SNAPSHOT_VERSION);
    assert!(snap.git_sha.is_none());
    assert!(snap.delta.is_none());
}

#[test]
fn reader_skips_unrelated_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README.md"), "not a segment").unwrap();
    std::fs::write(dir.path().join("2026-13.jsonl"), "invalid month").unwrap();
    let reader = HistoryReader::new(dir.path());
    assert_eq!(reader.segments().unwrap().len(), 0);
}
