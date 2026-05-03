//! `heal mark-fixed --finding-id ... --commit-sha ...` — record (or
//! refresh) a `FixedFinding` entry in `.heal/checks/fixed.json`.
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

use crate::core::check_cache::{upsert_fixed, FixedFinding};
use crate::core::HealPaths;

pub fn run(project: &Path, finding_id: &str, commit_sha: &str, as_json: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    let entry = FixedFinding {
        finding_id: finding_id.to_owned(),
        commit_sha: commit_sha.to_owned(),
        fixed_at: Utc::now(),
    };
    let path = paths.checks_fixed();
    upsert_fixed(&path, entry.clone())?;
    if as_json {
        #[derive(Serialize)]
        struct MarkReport<'a> {
            finding_id: &'a str,
            commit_sha: &'a str,
            fixed_at: String,
            path: String,
        }
        super::emit_json(&MarkReport {
            finding_id,
            commit_sha,
            fixed_at: entry.fixed_at.to_rfc3339(),
            path: path.display().to_string(),
        });
        return Ok(());
    }
    println!(
        "marked {finding_id} as fixed by {commit_sha} (recorded in {})",
        path.display(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::check_cache::read_fixed;
    use tempfile::TempDir;

    #[test]
    fn upserts_entries_keyed_by_finding_id() {
        let tmp = TempDir::new().unwrap();
        let paths = HealPaths::new(tmp.path());
        paths.ensure().unwrap();

        run(tmp.path(), "ccn:src/a.rs:foo:abc", "deadbeef", false).unwrap();
        run(tmp.path(), "ccn:src/b.rs:bar:def", "cafebabe", false).unwrap();

        let map = read_fixed(&paths.checks_fixed()).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map["ccn:src/a.rs:foo:abc"].commit_sha, "deadbeef");
        assert_eq!(map["ccn:src/b.rs:bar:def"].commit_sha, "cafebabe");
    }

    #[test]
    fn refreshes_existing_entry_for_same_finding_id() {
        let tmp = TempDir::new().unwrap();
        let paths = HealPaths::new(tmp.path());
        paths.ensure().unwrap();

        run(tmp.path(), "ccn:src/a.rs:foo:abc", "old", false).unwrap();
        run(tmp.path(), "ccn:src/a.rs:foo:abc", "new", false).unwrap();
        let map = read_fixed(&paths.checks_fixed()).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map["ccn:src/a.rs:foo:abc"].commit_sha, "new");
    }
}
