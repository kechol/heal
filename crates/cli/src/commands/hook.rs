use std::io::{IsTerminal, Read};
use std::path::Path;

use anyhow::Result;
use heal_core::history::{HistoryWriter, Snapshot};
use heal_core::HealPaths;

use crate::cli::HookEvent;
use crate::snapshot;

pub fn run(project: &Path, event: HookEvent) -> Result<()> {
    let paths = HealPaths::new(project);
    let writer = HistoryWriter::new(paths.history_dir());

    let payload = match event {
        HookEvent::Commit => capture_commit(project)?,
        HookEvent::Edit | HookEvent::Stop => capture_stdin()?,
    };

    writer.append(&Snapshot::new(event.as_str(), payload))?;
    Ok(())
}

fn capture_commit(project: &Path) -> Result<serde_json::Value> {
    let snap = snapshot::capture(project)?;
    Ok(serde_json::to_value(snap).expect("MetricsSnapshot serialization is infallible"))
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
