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

use std::io::{IsTerminal, Read, Write};
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use heal_core::config::{Config, PolicyConfig};
use heal_core::eventlog::{Event, EventLog};
use heal_core::snapshot::MetricsSnapshot;
use heal_core::state::State;
use heal_core::HealPaths;

use crate::cli::{CheckSkill, HookEvent};
use crate::finding::{derive_findings, Finding, Severity};
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
            // silent. The user-facing nudge lives in SessionStart.
            logs.append(&Event::new(event.as_str(), capture_stdin()?))?;
        }
        HookEvent::SessionStart => {
            run_session_start(project, &paths, &logs, &mut std::io::stdout().lock())?;
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

/// `SessionStart` entry point: append the raw payload to `logs/`, then read
/// the latest `MetricsSnapshot`, derive findings, filter by per-rule
/// cool-down, and emit a nudge to `out`. The state file (`runtime/state.json`)
/// is updated only when at least one finding actually fires so a silent
/// session leaves `last_fired` untouched.
fn run_session_start(
    project: &Path,
    paths: &HealPaths,
    logs: &EventLog,
    out: &mut dyn Write,
) -> Result<()> {
    logs.append(&Event::new(
        HookEvent::SessionStart.as_str(),
        capture_stdin()?,
    ))?;

    // A missing config is a normal state during `heal init` first-run, and
    // a missing snapshot dir means no commit has happened yet — both are
    // silent (return early). The nudge is best-effort.
    let cfg = match heal_core::config::load_from_project(project) {
        Ok(c) => c,
        Err(heal_core::Error::ConfigMissing(_)) => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let snap_log = EventLog::new(paths.snapshots_dir());
    let Some((_, snapshot)) = MetricsSnapshot::latest_in(&snap_log)? else {
        return Ok(());
    };
    let findings = derive_findings(&snapshot, &cfg);
    if findings.is_empty() {
        return Ok(());
    }

    let mut state = State::load(&paths.state()).unwrap_or_default();
    let now = Utc::now();
    let fresh: Vec<Finding> = findings
        .into_iter()
        .filter(|f| !is_in_cooldown(&state, f, &cfg, now))
        .collect();
    if fresh.is_empty() {
        return Ok(());
    }
    for f in &fresh {
        state.last_fired.insert(f.cooldown_key(), now);
    }
    state.save(&paths.state())?;

    write_nudge(out, &fresh)?;
    Ok(())
}

fn is_in_cooldown(state: &State, finding: &Finding, cfg: &Config, now: DateTime<Utc>) -> bool {
    let Some(prev) = state.last_fired.get(&finding.cooldown_key()) else {
        return false;
    };
    let hours = cooldown_hours_for(cfg, &finding.rule_id);
    let elapsed = now - *prev;
    elapsed < Duration::hours(i64::from(hours))
}

/// Look up the cool-down for a rule. Falls back to the global default
/// (`PolicyConfig::default cooldown_hours`, currently 24h) when the rule
/// has no `[policy.<rule>]` section.
fn cooldown_hours_for(cfg: &Config, rule_id: &str) -> u32 {
    cfg.policy
        .get(rule_id)
        .map_or(24, |p: &PolicyConfig| p.cooldown_hours)
}

fn write_nudge(out: &mut dyn Write, findings: &[Finding]) -> Result<()> {
    writeln!(out, "HEAL: {} finding(s) need attention.", findings.len())?;
    for f in findings {
        let badge = match f.severity {
            Severity::Warn => "warn",
            Severity::Info => "info",
        };
        writeln!(out, "  [{badge}] {}", f.message)?;
    }
    writeln!(out)?;
    writeln!(
        out,
        "Run `heal check` for synthesis with refactor proposals."
    )?;
    let drilldowns = drilldown_skills(findings);
    if !drilldowns.is_empty() {
        let cmds: Vec<String> = drilldowns
            .iter()
            .map(|s| format!("`heal check {s}`"))
            .collect();
        writeln!(out, "Drill in: {}.", cmds.join(", "))?;
    }
    Ok(())
}

/// Map the rule ids surfaced this turn onto the relevant per-metric
/// `check-*` short names. Returns the unique set in a stable order so
/// the nudge output is deterministic across runs.
fn drilldown_skills(findings: &[Finding]) -> Vec<&'static str> {
    use std::collections::BTreeSet;
    let mut skills = BTreeSet::new();
    for f in findings {
        if let Some(s) = CheckSkill::for_rule(&f.rule_id) {
            skills.insert(s.short_name());
        }
    }
    skills.into_iter().collect()
}

/// Lightweight snapshot of the just-recorded commit (sha, parent, author,
/// subject, file/line change counts). Pure metadata — the heavy metric
/// payload lives in `snapshots/`. A failed lookup is logged to stderr but
/// returns `Value::Null` so the post-commit hook never aborts the commit.
fn commit_log_payload(project: &Path) -> serde_json::Value {
    let Some(info) = heal_observer::git::head_commit_info(project) else {
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
        let info: heal_observer::git::CommitInfo =
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

    #[test]
    fn session_start_silent_without_snapshot() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        std::fs::write(paths.config(), "").unwrap();

        let mut buf: Vec<u8> = Vec::new();
        run_session_start(
            dir.path(),
            &paths,
            &EventLog::new(paths.logs_dir()),
            &mut buf,
        )
        .unwrap();

        assert!(
            buf.is_empty(),
            "expected no nudge: {}",
            String::from_utf8_lossy(&buf)
        );
        // logs always pick up the event regardless.
        assert_eq!(read_log_events(&paths).len(), 1);
    }

    #[test]
    fn session_start_emits_nudge_for_fresh_findings() {
        use heal_core::snapshot::{HotspotDelta, SnapshotDelta};

        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        std::fs::write(paths.config(), "").unwrap();

        // Plant a snapshot with a hotspot.new_top finding.
        let snap = MetricsSnapshot {
            delta: Some(
                serde_json::to_value(SnapshotDelta {
                    hotspot: Some(HotspotDelta {
                        max_score: 1.0,
                        top_files_added: vec!["src/auth/session.ts".into()],
                        top_files_dropped: vec![],
                    }),
                    ..SnapshotDelta::default()
                })
                .unwrap(),
            ),
            ..MetricsSnapshot::default()
        };
        EventLog::new(paths.snapshots_dir())
            .append(&Event::new("commit", serde_json::to_value(&snap).unwrap()))
            .unwrap();

        let mut buf: Vec<u8> = Vec::new();
        run_session_start(
            dir.path(),
            &paths,
            &EventLog::new(paths.logs_dir()),
            &mut buf,
        )
        .unwrap();

        let stdout = String::from_utf8(buf).unwrap();
        assert!(stdout.contains("HEAL"));
        assert!(stdout.contains("src/auth/session.ts"));
        // last_fired must be persisted under the new runtime/ path.
        let state = State::load(&paths.state()).unwrap();
        assert!(state
            .last_fired
            .keys()
            .any(|k| k == "hotspot.new_top:src/auth/session.ts"));
    }

    #[test]
    fn session_start_respects_cooldown() {
        use heal_core::snapshot::{HotspotDelta, SnapshotDelta};

        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        std::fs::write(paths.config(), "").unwrap();

        let snap = MetricsSnapshot {
            delta: Some(
                serde_json::to_value(SnapshotDelta {
                    hotspot: Some(HotspotDelta {
                        max_score: 1.0,
                        top_files_added: vec!["src/x.rs".into()],
                        top_files_dropped: vec![],
                    }),
                    ..SnapshotDelta::default()
                })
                .unwrap(),
            ),
            ..MetricsSnapshot::default()
        };
        EventLog::new(paths.snapshots_dir())
            .append(&Event::new("commit", serde_json::to_value(&snap).unwrap()))
            .unwrap();

        // Pre-seed last_fired within the 24h cool-down window.
        let mut state = State::default();
        state
            .last_fired
            .insert("hotspot.new_top:src/x.rs".into(), Utc::now());
        state.save(&paths.state()).unwrap();

        let mut buf: Vec<u8> = Vec::new();
        run_session_start(
            dir.path(),
            &paths,
            &EventLog::new(paths.logs_dir()),
            &mut buf,
        )
        .unwrap();

        assert!(
            buf.is_empty(),
            "cool-down should suppress nudge: {}",
            String::from_utf8_lossy(&buf)
        );
    }
}
