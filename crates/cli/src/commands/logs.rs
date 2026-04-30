//! `heal logs` / `heal snapshots` / `heal checks` — three sibling
//! browsers over the append-only stores under `.heal/`.
//!
//! - `heal logs`      → `.heal/logs/` (commit / edit / stop hook events)
//! - `heal snapshots` → `.heal/snapshots/` (commit `MetricsSnapshot` +
//!   `calibrate` events)
//! - `heal checks`    → `.heal/checks/` (`CheckRecord` log)
//!
//! The first two share an `EventLog`-shaped reader and the same
//! `--since` / `--filter` / `--limit` / `--json` filter set; `heal
//! checks` uses `CheckRecord` reads via [`iter_records`] and omits
//! `--filter` (no event-name dimension to match).

use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::cli::{ChecksFilters, LogFilters};
use crate::core::check_cache::{iter_records, CheckRecord};
use crate::core::eventlog::{Event, EventLog};
use crate::core::HealPaths;

pub fn run_logs(project: &Path, args: &LogFilters) -> Result<()> {
    run_eventlog(HealPaths::new(project).logs_dir(), args)
}

pub fn run_snapshots(project: &Path, args: &LogFilters) -> Result<()> {
    run_eventlog(HealPaths::new(project).snapshots_dir(), args)
}

pub fn run_checks(project: &Path, args: &ChecksFilters) -> Result<()> {
    let paths = HealPaths::new(project);
    let since_dt = parse_since(args.since.as_deref())?;
    let mut records: Vec<CheckRecord> = iter_records(&paths.checks_dir())?
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| since_dt.is_none_or(|cutoff| r.started_at >= cutoff))
        .collect();
    if let Some(n) = args.limit {
        records.truncate(n);
    }
    if args.json {
        let payload: Vec<ChecksRow> = records.iter().map(ChecksRow::from).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).expect("ChecksRow serialization is infallible")
        );
        return Ok(());
    }
    if records.is_empty() {
        println!(
            "no records yet at {} — run `heal check`",
            paths.checks_dir().display()
        );
        return Ok(());
    }
    for r in &records {
        let counts = &r.severity_counts;
        println!(
            "{}  {}  head={}  findings={}  C {}  H {}  M {}",
            r.check_id,
            r.started_at.format("%Y-%m-%d %H:%M"),
            r.head_sha.as_deref().unwrap_or("∅"),
            r.findings.len(),
            counts.critical,
            counts.high,
            counts.medium,
        );
    }
    Ok(())
}

fn run_eventlog(dir: PathBuf, args: &LogFilters) -> Result<()> {
    let log = EventLog::new(dir);
    let since_dt = parse_since(args.since.as_deref())?;

    let mut kept: Vec<Event> = Vec::new();
    for event in log.try_iter()? {
        let event = event?;
        if let Some(cutoff) = since_dt {
            if event.timestamp < cutoff {
                continue;
            }
        }
        if let Some(name) = args.filter.as_deref() {
            if event.event != name {
                continue;
            }
        }
        kept.push(event);
    }

    if let Some(n) = args.limit {
        if kept.len() > n {
            let drop = kept.len() - n;
            kept.drain(..drop);
        }
    }

    for event in &kept {
        if args.json {
            println!("{}", serde_json::to_string(event)?);
        } else {
            print_event(event);
        }
    }
    Ok(())
}

fn parse_since(since: Option<&str>) -> Result<Option<DateTime<Utc>>> {
    since
        .map(|s| {
            DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(anyhow::Error::from)
        })
        .transpose()
}

fn print_event(event: &Event) {
    let ts = event.timestamp.format("%Y-%m-%d %H:%M:%S UTC");
    println!("[{ts}] {}", event.event);
    if has_meaningful_data(&event.data) {
        let pretty =
            serde_json::to_string_pretty(&event.data).expect("Value serialization is infallible");
        for line in pretty.lines() {
            println!("    {line}");
        }
    }
}

fn has_meaningful_data(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        _ => true,
    }
}

/// JSON row for `heal checks --json`. Mirrors the fields a tool would
/// pull from a full `CheckRecord` for indexing/listing without paying
/// for the embedded findings list.
#[derive(Debug, Clone, Serialize)]
struct ChecksRow {
    check_id: String,
    started_at: DateTime<Utc>,
    head_sha: Option<String>,
    findings_count: usize,
    severity_counts: crate::core::snapshot::SeverityCounts,
    worktree_clean: bool,
}

impl From<&CheckRecord> for ChecksRow {
    fn from(r: &CheckRecord) -> Self {
        Self {
            check_id: r.check_id.clone(),
            started_at: r.started_at,
            head_sha: r.head_sha.clone(),
            findings_count: r.findings.len(),
            severity_counts: r.severity_counts,
            worktree_clean: r.worktree_clean,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;
    use tempfile::TempDir;

    fn args(filter: Option<&str>, since: Option<&str>, limit: Option<usize>) -> LogFilters {
        LogFilters {
            since: since.map(str::to_string),
            filter: filter.map(str::to_string),
            limit,
            json: true,
        }
    }

    fn write_events(paths: &HealPaths) {
        let log = EventLog::new(paths.logs_dir());
        log.append(&Event {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
            event: "commit".into(),
            data: json!({"sha": "aaa"}),
        })
        .unwrap();
        log.append(&Event {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 2, 0, 0, 0).unwrap(),
            event: "edit".into(),
            data: json!({"file": "main.rs"}),
        })
        .unwrap();
        log.append(&Event {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 3, 0, 0, 0).unwrap(),
            event: "stop".into(),
            data: json!(null),
        })
        .unwrap();
    }

    fn collect(paths: &HealPaths, filter: Option<&str>) -> Vec<Event> {
        let log = EventLog::new(paths.logs_dir());
        log.try_iter()
            .unwrap()
            .map(|r| r.unwrap())
            .filter(|e| filter.is_none_or(|f| e.event == f))
            .collect()
    }

    #[test]
    fn smoke_run_succeeds_on_populated_logs() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        write_events(&paths);
        run_logs(dir.path(), &args(None, None, None)).unwrap();
        assert_eq!(collect(&paths, None).len(), 3);
    }

    #[test]
    fn filter_keeps_only_matching_event_name() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        write_events(&paths);
        let edits = collect(&paths, Some("edit"));
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].event, "edit");
    }

    #[test]
    fn since_parses_rfc3339() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        write_events(&paths);
        run_logs(dir.path(), &args(None, Some("2026-04-02T12:00:00Z"), None)).unwrap();
    }

    #[test]
    fn run_returns_ok_when_logs_dir_missing() {
        let dir = TempDir::new().unwrap();
        let mut a = args(None, None, None);
        a.json = false;
        run_logs(dir.path(), &a).unwrap();
    }

    #[test]
    fn run_snapshots_reads_snapshots_dir() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        // Drop a `calibrate` event into snapshots/ to confirm the
        // dispatcher is reading the right directory.
        EventLog::new(paths.snapshots_dir())
            .append(&Event {
                timestamp: Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
                event: "calibrate".into(),
                data: json!({"reason": "manual"}),
            })
            .unwrap();
        run_snapshots(dir.path(), &args(None, None, None)).unwrap();
    }

    #[test]
    fn run_checks_handles_empty_dir() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        run_checks(
            dir.path(),
            &ChecksFilters {
                since: None,
                limit: None,
                json: false,
            },
        )
        .unwrap();
    }

    #[test]
    fn print_event_skips_empty_string_data() {
        // Sanity: a non-null but blank string shouldn't render an indent line.
        assert!(!has_meaningful_data(&Value::String(String::new())));
        assert!(has_meaningful_data(&Value::String("x".into())));
        assert!(!has_meaningful_data(&Value::Null));
        assert!(has_meaningful_data(&json!({"k": "v"})));
        assert!(!has_meaningful_data(&json!({})));
    }
}
