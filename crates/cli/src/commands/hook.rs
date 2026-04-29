//! `heal hook <commit|edit|stop>` — single entrypoint invoked by git hooks
//! and the Claude plugin. Each event has a different write target:
//!
//! | event  | snapshots/ | logs/ | observer scan | stdin payload |
//! | ------ | :--------: | :---: | :-----------: | :-----------: |
//! | commit |     ✓      |   ✓   |       ✓       |       —       |
//! | edit   |     —      |   ✓   |       —       |       ✓       |
//! | stop   |     —      |   ✓   |       —       |       ✓       |
//!
//! `commit` is the only event that runs observers (heavy work) — `edit` and
//! `stop` stay below ~1ms so the Claude plugin loop isn't slowed down.
//! `commit` also writes a lightweight metadata record to `logs/` so the
//! event timeline (`heal logs`) is the single source of truth for "what
//! happened when", while `snapshots/` retains the typed metric series.
//!
//! The v0.1 `SessionStart` nudge that lived here was retired in v0.2; a
//! Severity-aware post-commit nudge will hang off `run_commit` once
//! Calibration lands (TODO §post-commit nudge).

use std::io::{IsTerminal, Read};
use std::path::Path;

use crate::core::eventlog::{Event, EventLog};
use crate::core::HealPaths;
use anyhow::Result;

use crate::cli::HookEvent;
use crate::snapshot;

pub fn run(project: &Path, event: HookEvent) -> Result<()> {
    let paths = HealPaths::new(project);
    let logs = EventLog::new(paths.logs_dir());

    match event {
        HookEvent::Commit => run_commit(project, &paths, &logs)?,
        HookEvent::Edit | HookEvent::Stop => {
            // Both events stay log-only. Stop intentionally does NOT emit a
            // nudge: `MetricsSnapshot` only updates on commit, so any
            // turn-level Stop nudge would either repeat itself or stay
            // silent.
            logs.append(&Event::new(event.as_str(), capture_stdin()?))?;
        }
    }
    Ok(())
}

fn run_commit(project: &Path, paths: &HealPaths, logs: &EventLog) -> Result<()> {
    let metrics_payload = snapshot::capture_value(project)?;
    EventLog::new(paths.snapshots_dir())
        .append(&Event::new(HookEvent::Commit.as_str(), metrics_payload))?;
    logs.append(&Event::new(
        HookEvent::Commit.as_str(),
        commit_log_payload(project),
    ))?;
    Ok(())
}

/// Lightweight snapshot of the just-recorded commit (sha, parent, author,
/// subject, file/line change counts). Pure metadata — the heavy metric
/// payload lives in `snapshots/`. A failed lookup is logged to stderr but
/// returns `Value::Null` so the post-commit hook never aborts the commit.
fn commit_log_payload(project: &Path) -> serde_json::Value {
    let Some(info) = crate::observer::git::head_commit_info(project) else {
        eprintln!("heal: commit metadata unavailable (HEAD missing or not a git repo)");
        return serde_json::Value::Null;
    };
    serde_json::to_value(&info).expect("CommitInfo serialization is infallible")
}

fn capture_stdin() -> Result<serde_json::Value> {
    // Claude plugin hooks deliver event metadata via stdin (JSON). Skip the
    // read on a tty so manual invocations don't block on user input.
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Ok(serde_json::Value::Null);
    }
    let mut buf = String::new();
    stdin.lock().read_to_string(&mut buf)?;
    if buf.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    Ok(match serde_json::from_str(&buf) {
        Ok(v) => v,
        Err(_) => serde_json::Value::String(buf),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{commit, init_repo};
    use tempfile::TempDir;

    fn read_log_events(paths: &HealPaths) -> Vec<Event> {
        EventLog::new(paths.logs_dir())
            .try_iter()
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    #[test]
    fn commit_writes_to_both_snapshots_and_logs() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(
            dir.path(),
            "lib.rs",
            "fn ok() {}\n",
            "alice@example.com",
            "feat: add ok",
        );
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        // Commit hook needs `.heal/config.toml` to drive observers; init flow
        // would normally create it, but we exercise the hook in isolation.
        std::fs::write(paths.config(), "").unwrap();

        run(dir.path(), HookEvent::Commit).unwrap();

        let snap_events: Vec<Event> = EventLog::new(paths.snapshots_dir())
            .try_iter()
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(snap_events.len(), 1);
        assert_eq!(snap_events[0].event, HookEvent::Commit.as_str());

        let log_events = read_log_events(&paths);
        assert_eq!(log_events.len(), 1);
        assert_eq!(log_events[0].event, HookEvent::Commit.as_str());
        let info: crate::observer::git::CommitInfo =
            serde_json::from_value(log_events[0].data.clone()).unwrap();
        assert_eq!(info.author_email.as_deref(), Some("alice@example.com"));
        assert_eq!(info.message_summary, "feat: add ok");
        assert_eq!(info.files_changed, 1);
        assert!(info.insertions >= 1);
    }

    #[test]
    fn edit_only_writes_to_logs() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        run(dir.path(), HookEvent::Edit).unwrap();

        let snap_files: usize = std::fs::read_dir(paths.snapshots_dir()).unwrap().count();
        assert_eq!(snap_files, 0);

        let log_events = read_log_events(&paths);
        assert_eq!(log_events.len(), 1);
        assert_eq!(log_events[0].event, HookEvent::Edit.as_str());
        assert!(log_events[0].data.is_null());
    }

    #[test]
    fn stop_only_writes_to_logs() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        run(dir.path(), HookEvent::Stop).unwrap();
        let log_events = read_log_events(&paths);
        assert_eq!(log_events.len(), 1);
        assert_eq!(log_events[0].event, HookEvent::Stop.as_str());
    }
}
