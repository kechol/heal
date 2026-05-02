//! `heal fix *` — operations on the fix-tracking state attached to
//! `.heal/checks/`. Read-only browsing of the record stream lives at
//! `heal checks`; this command surface focuses on the per-record /
//! per-finding actions a fix workflow needs.
//!
//! Sub-commands:
//! - `fix show <id>`  — detailed render of one record (unstable view;
//!   use `--json` for the stable shape).
//! - `fix mark`       — append a `FixedFinding` line to
//!   `.heal/checks/fixed.jsonl` (the only `fix` command that writes).

use std::io::IsTerminal;
use std::path::Path;

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;

use crate::cli::FixAction;
use crate::commands::status::{render, Filters};
use crate::core::check_cache::{append_fixed, find_by_id, iter_records, FixedFinding};
use crate::core::HealPaths;

pub fn run(project: &Path, action: FixAction) -> Result<()> {
    let paths = HealPaths::new(project);
    match action {
        FixAction::Show { check_id, json } => run_show(&paths, &check_id, json),
        FixAction::Mark {
            finding_id,
            commit_sha,
            json,
        } => run_mark(&paths, &finding_id, &commit_sha, json),
    }
}

fn run_mark(paths: &HealPaths, finding_id: &str, commit_sha: &str, as_json: bool) -> Result<()> {
    let entry = FixedFinding {
        finding_id: finding_id.to_owned(),
        commit_sha: commit_sha.to_owned(),
        fixed_at: Utc::now(),
    };
    let log_path = paths.checks_fixed_log();
    append_fixed(&log_path, &entry)?;
    if as_json {
        #[derive(Serialize)]
        struct MarkReport<'a> {
            action: &'a str,
            finding_id: &'a str,
            commit_sha: &'a str,
            fixed_at: String,
            log: String,
        }
        super::emit_json(&MarkReport {
            action: "marked",
            finding_id,
            commit_sha,
            fixed_at: entry.fixed_at.to_rfc3339(),
            log: log_path.display().to_string(),
        });
        return Ok(());
    }
    println!(
        "marked {finding_id} as fixed by {commit_sha} (logged to {})",
        log_path.display(),
    );
    Ok(())
}

fn run_show(paths: &HealPaths, check_id: &str, as_json: bool) -> Result<()> {
    let records = iter_records(&paths.checks_dir())?;
    let record = find_by_id(&records, check_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no CheckRecord with check_id={check_id}"))?;
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&record).expect("CheckRecord serialization is infallible")
        );
        return Ok(());
    }
    eprintln!(
        "warning: `heal fix show` rendering is unstable; use `--json` for a stable contract.",
    );
    let mut stdout = std::io::stdout();
    let colorize = stdout.is_terminal();
    // Show full detail — turn on `--all` semantics so Medium/Ok aren't hidden.
    let filters = Filters {
        all: true,
        ..Filters::default()
    };
    let cfg = crate::core::config::Config::default();
    render(&record, &[], &filters, &cfg, colorize, &mut stdout)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::check_cache::read_fixed;
    use tempfile::TempDir;

    #[test]
    fn mark_appends_entry_with_supplied_metadata() {
        let tmp = TempDir::new().unwrap();
        let paths = HealPaths::new(tmp.path());
        paths.ensure().unwrap();

        run_mark(&paths, "ccn:src/a.rs:foo:abc", "deadbeef", false).unwrap();
        run_mark(&paths, "ccn:src/b.rs:bar:def", "cafebabe", false).unwrap();

        let entries = read_fixed(&paths.checks_fixed_log()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].finding_id, "ccn:src/a.rs:foo:abc");
        assert_eq!(entries[0].commit_sha, "deadbeef");
        assert_eq!(entries[1].finding_id, "ccn:src/b.rs:bar:def");
        assert_eq!(entries[1].commit_sha, "cafebabe");
    }
}
