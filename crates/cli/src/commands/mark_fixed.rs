//! `heal mark-fixed --finding-id ... --commit-sha ...` — append a
//! `FixedFinding` to `.heal/checks/fixed.jsonl`.
//!
//! Agent-facing surface. The `/heal-code-patch` skill calls this after
//! committing a fix so the next `heal status --refresh` either retires
//! the entry (genuinely fixed) or moves it to `regressed.jsonl`. Hidden
//! from the top-level `--help` so it doesn't pollute the user-facing
//! command list — humans never invoke this directly.

use std::path::Path;

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;

use crate::core::check_cache::{append_fixed, FixedFinding};
use crate::core::HealPaths;

pub fn run(project: &Path, finding_id: &str, commit_sha: &str, as_json: bool) -> Result<()> {
    let paths = HealPaths::new(project);
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
            finding_id: &'a str,
            commit_sha: &'a str,
            fixed_at: String,
            log: String,
        }
        super::emit_json(&MarkReport {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::check_cache::read_fixed;
    use tempfile::TempDir;

    #[test]
    fn appends_entry_with_supplied_metadata() {
        let tmp = TempDir::new().unwrap();
        let paths = HealPaths::new(tmp.path());
        paths.ensure().unwrap();

        run(tmp.path(), "ccn:src/a.rs:foo:abc", "deadbeef", false).unwrap();
        run(tmp.path(), "ccn:src/b.rs:bar:def", "cafebabe", false).unwrap();

        let entries = read_fixed(&paths.checks_fixed_log()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].finding_id, "ccn:src/a.rs:foo:abc");
        assert_eq!(entries[0].commit_sha, "deadbeef");
        assert_eq!(entries[1].finding_id, "ccn:src/b.rs:bar:def");
        assert_eq!(entries[1].commit_sha, "cafebabe");
    }
}
