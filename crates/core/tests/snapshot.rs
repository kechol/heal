use chrono::{Datelike, TimeZone, Utc};
use heal_core::eventlog::{Event, EventLog};
use heal_core::snapshot::{MetricsSnapshot, METRICS_SNAPSHOT_VERSION};
use serde_json::json;

#[test]
fn metrics_snapshot_round_trips_through_writer() {
    let dir = tempfile::tempdir().unwrap();
    let log = EventLog::new(dir.path());

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
    let event = Event {
        timestamp: Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap(),
        event: "commit".into(),
        data: serde_json::to_value(&metrics).unwrap(),
    };
    log.append(&event).unwrap();

    let events: Vec<_> = log.try_iter().unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(events.len(), 1);
    let decoded: MetricsSnapshot = serde_json::from_value(events[0].data.clone()).unwrap();
    assert_eq!(decoded, metrics);
}

#[test]
fn latest_in_returns_most_recent() {
    let dir = tempfile::tempdir().unwrap();
    let log = EventLog::new(dir.path());
    let mk = |month: u32, sha: &str| -> Event {
        let m = MetricsSnapshot {
            git_sha: Some(sha.into()),
            ..MetricsSnapshot::default()
        };
        Event {
            timestamp: Utc.with_ymd_and_hms(2026, month, 1, 0, 0, 0).unwrap(),
            event: "commit".into(),
            data: serde_json::to_value(&m).unwrap(),
        }
    };
    log.append(&mk(2, "feb")).unwrap();
    log.append(&mk(3, "mar")).unwrap();
    log.append(&mk(4, "apr")).unwrap();

    let (event, metrics) = MetricsSnapshot::latest_in(&log).unwrap().unwrap();
    assert_eq!(event.timestamp.month(), 4);
    assert_eq!(metrics.git_sha.as_deref(), Some("apr"));
}

#[test]
fn latest_in_skips_legacy_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let log = EventLog::new(dir.path());

    let real = MetricsSnapshot {
        git_sha: Some("real".into()),
        ..MetricsSnapshot::default()
    };
    log.append(&Event {
        timestamp: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
        event: "commit".into(),
        data: serde_json::to_value(&real).unwrap(),
    })
    .unwrap();
    // Legacy payload from a pre-MetricsSnapshot binary, written *after* the
    // real snapshot — must be skipped so the real one is returned.
    log.append(&Event {
        timestamp: Utc.with_ymd_and_hms(2026, 4, 2, 0, 0, 0).unwrap(),
        event: "edit".into(),
        data: json!("raw stdin string"),
    })
    .unwrap();

    let (_, metrics) = MetricsSnapshot::latest_in(&log).unwrap().unwrap();
    assert_eq!(metrics.git_sha.as_deref(), Some("real"));
}

#[test]
fn latest_in_empty_dir_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let log = EventLog::new(dir.path().join("missing"));
    assert!(MetricsSnapshot::latest_in(&log).unwrap().is_none());
}

#[test]
fn metrics_snapshot_default_version_is_current() {
    let snap = MetricsSnapshot::default();
    assert_eq!(snap.version, METRICS_SNAPSHOT_VERSION);
    assert!(snap.git_sha.is_none());
    assert!(snap.delta.is_none());
}
