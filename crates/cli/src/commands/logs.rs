//! `heal logs` — browse `.heal/logs/` entries written by the commit / edit /
//! stop hooks. The corresponding metric snapshots live in `.heal/snapshots/`
//! and are surfaced via `heal status`; this command only walks the event
//! timeline.

use std::path::Path;

use crate::core::eventlog::{Event, EventLog};
use crate::core::HealPaths;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::cli::LogsArgs;

pub fn run(project: &Path, args: &LogsArgs) -> Result<()> {
    let paths = HealPaths::new(project);
    let log = EventLog::new(paths.logs_dir());

    let since_dt: Option<DateTime<Utc>> = args
        .since
        .as_deref()
        .map(|s| {
            DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(anyhow::Error::from)
        })
        .transpose()?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;
    use tempfile::TempDir;

    fn args(filter: Option<&str>, since: Option<&str>, limit: Option<usize>) -> LogsArgs {
        LogsArgs {
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
        run(dir.path(), &args(None, None, None)).unwrap();
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
        run(dir.path(), &args(None, Some("2026-04-02T12:00:00Z"), None)).unwrap();
    }

    #[test]
    fn run_returns_ok_when_logs_dir_missing() {
        let dir = TempDir::new().unwrap();
        let mut a = args(None, None, None);
        a.json = false;
        run(dir.path(), &a).unwrap();
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
