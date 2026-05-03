//! `heal mark <action>` — single agent-facing entrypoint for the
//! per-finding state machines outside of `heal status`.
//!
//! Two actions live here:
//!
//! - `mark fix` (skill-driven): the `/heal-code-patch` flow records a
//!   fix into `.heal/findings/fixed.json` after each commit so the
//!   next `heal status --refresh` either retires the entry (genuinely
//!   fixed) or moves it to `regressed.jsonl` (re-detected).
//! - `mark accept` (skill-driven): the `/heal-code-review` flow
//!   records "this Finding is intrinsic / cohesive procedural / a
//!   load-bearing boundary; stop surfacing it in the drain queue"
//!   into `.heal/findings/accepted.json`. Distinct from `fix` —
//!   accepted entries persist across re-detections by design.
//!
//! Both subcommands are hidden from `--help`: humans drive them via
//! their respective skills, and surfacing them as top-level commands
//! invites running them without the surrounding workflow that gives
//! the entry meaning (a commit for `fix`, a documented `reason` for
//! `accept`).
//!
//! `heal mark-fixed` (the v0.2 surface) is kept as a hidden alias
//! that delegates to `mark fix` with a one-line stderr deprecation
//! warning. The `/heal-code-patch` skill bundle is updated to call
//! `heal mark fix` directly; users on stale skill copies see the
//! warning until they run `heal skills update`.

use std::path::Path;

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::Serialize;

use crate::core::accepted::{snapshot, upsert_accepted, AcceptedFinding};
use crate::core::findings_cache::{read_latest, upsert_fixed, FixedFinding};
use crate::core::HealPaths;
use crate::observer::git;

/// `heal mark fix --finding-id <ID> --commit-sha <SHA>`. Upserts a
/// `FixedFinding` into `.heal/findings/fixed.json` so the next
/// `heal status --refresh` reconciles it.
pub fn run_fix(project: &Path, finding_id: &str, commit_sha: &str, as_json: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    let entry = FixedFinding {
        finding_id: finding_id.to_owned(),
        commit_sha: commit_sha.to_owned(),
        fixed_at: Utc::now(),
    };
    let path = paths.findings_fixed();
    upsert_fixed(&path, entry.clone())?;
    if as_json {
        #[derive(Serialize)]
        struct FixReport<'a> {
            finding_id: &'a str,
            commit_sha: &'a str,
            fixed_at: String,
            path: String,
        }
        super::emit_json(&FixReport {
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

/// `heal mark fix --finding-id ...` invoked via the deprecated
/// `heal mark-fixed` alias. Same effect; prints a one-line warning
/// to stderr so users on stale skills know to upgrade.
pub fn run_fix_legacy(
    project: &Path,
    finding_id: &str,
    commit_sha: &str,
    as_json: bool,
) -> Result<()> {
    eprintln!(
        "warning: `heal mark-fixed` is deprecated. Use `heal mark fix --finding-id <ID> --commit-sha <SHA>`."
    );
    eprintln!("         To refresh bundled skills: `heal skills update`");
    run_fix(project, finding_id, commit_sha, as_json)
}

/// `heal mark accept --finding-id <ID> --reason <TEXT>`. Snapshots
/// the live finding (severity, hotspot, `metric_value`, summary)
/// and inserts an `AcceptedFinding` into
/// `.heal/findings/accepted.json`. The finding must be present in
/// `latest.json` — accepting an id we can't see makes the snapshot
/// fields meaningless, and is almost always a stale-id typo.
pub fn run_accept(project: &Path, finding_id: &str, reason: &str, as_json: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    let record = read_latest(&paths.findings_latest())?.ok_or_else(|| {
        anyhow!(
            "no findings cache at {} — run `heal status --refresh` first",
            paths.findings_latest().display(),
        )
    })?;
    let finding = record
        .findings
        .iter()
        .find(|f| f.id == finding_id)
        .ok_or_else(|| {
            anyhow!(
                "no finding with id `{finding_id}` in {} — run `heal status --refresh` to resync",
                paths.findings_latest().display(),
            )
        })?;
    let accepted_at = Utc::now();
    let accepted_by = git::user_signature(project);
    let entry = snapshot(finding, reason.to_owned(), accepted_at, accepted_by);
    let path = paths.findings_accepted();
    upsert_accepted(&path, finding_id, entry.clone())?;
    if as_json {
        super::emit_json(&AcceptReport {
            finding_id,
            entry: &entry,
            path: path.display().to_string(),
        });
        return Ok(());
    }
    println!(
        "marked {finding_id} as accepted ({}) (recorded in {})",
        finding.metric,
        path.display(),
    );
    Ok(())
}

#[derive(Serialize)]
struct AcceptReport<'a> {
    finding_id: &'a str,
    #[serde(flatten)]
    entry: &'a AcceptedFinding,
    path: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::accepted::read_accepted;
    use crate::core::finding::{Finding, Location};
    use crate::core::findings_cache::{read_fixed, write_record, FindingsRecord};
    use crate::core::severity::Severity;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn finding(id_hint: &str, severity: Severity) -> Finding {
        let mut f = Finding::new(
            "ccn",
            Location::file(PathBuf::from(format!("src/{id_hint}.ts"))),
            "CCN=12 foo".into(),
            id_hint,
        );
        f.severity = severity;
        f
    }

    fn seed_record(paths: &HealPaths, findings: Vec<Finding>) {
        let rec = FindingsRecord::new(Some("sha".into()), true, "h".into(), findings);
        write_record(&paths.findings_latest(), &rec).unwrap();
    }

    #[test]
    fn fix_upserts_entry() {
        let tmp = TempDir::new().unwrap();
        let paths = HealPaths::new(tmp.path());
        paths.ensure().unwrap();

        run_fix(tmp.path(), "id-a", "deadbeef", false).unwrap();
        run_fix(tmp.path(), "id-b", "cafebabe", false).unwrap();

        let map = read_fixed(&paths.findings_fixed()).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map["id-a"].commit_sha, "deadbeef");
    }

    #[test]
    fn fix_legacy_prints_warning_and_succeeds() {
        // Behavioral: the legacy path delegates to the new path. The
        // stderr deprecation line is observable via cargo test's
        // captured output but we don't assert on it directly to keep
        // the test deterministic across env tweaks.
        let tmp = TempDir::new().unwrap();
        let paths = HealPaths::new(tmp.path());
        paths.ensure().unwrap();

        run_fix_legacy(tmp.path(), "id-a", "deadbeef", false).unwrap();
        let map = read_fixed(&paths.findings_fixed()).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map["id-a"].commit_sha, "deadbeef");
    }

    #[test]
    fn accept_snapshots_finding_state() {
        let tmp = TempDir::new().unwrap();
        let paths = HealPaths::new(tmp.path());
        paths.ensure().unwrap();
        let f = finding("alpha", Severity::Critical);
        let f_id = f.id.clone();
        seed_record(&paths, vec![f]);

        run_accept(tmp.path(), &f_id, "intrinsic dispatcher", false).unwrap();

        let map = read_accepted(&paths.findings_accepted()).unwrap();
        assert_eq!(map.len(), 1);
        let entry = &map[&f_id];
        assert_eq!(entry.reason, "intrinsic dispatcher");
        assert_eq!(entry.metric, "ccn");
        assert_eq!(entry.severity, Severity::Critical);
    }

    #[test]
    fn accept_allows_empty_reason() {
        // Per the design — empty reason is acceptable. The AI agent
        // driving the command is expected to fill it; we don't push
        // friction onto the CLI for the rare hand-invocation case.
        let tmp = TempDir::new().unwrap();
        let paths = HealPaths::new(tmp.path());
        paths.ensure().unwrap();
        let f = finding("alpha", Severity::High);
        let f_id = f.id.clone();
        seed_record(&paths, vec![f]);

        run_accept(tmp.path(), &f_id, "", false).unwrap();
        let map = read_accepted(&paths.findings_accepted()).unwrap();
        assert_eq!(map[&f_id].reason, "");
    }

    #[test]
    fn accept_errors_when_cache_missing() {
        let tmp = TempDir::new().unwrap();
        let paths = HealPaths::new(tmp.path());
        paths.ensure().unwrap();
        // No latest.json on disk.
        let res = run_accept(tmp.path(), "anything", "x", false);
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("heal status --refresh"));
    }

    #[test]
    fn accept_errors_when_id_unknown() {
        let tmp = TempDir::new().unwrap();
        let paths = HealPaths::new(tmp.path());
        paths.ensure().unwrap();
        seed_record(&paths, vec![finding("alpha", Severity::High)]);
        let res = run_accept(tmp.path(), "nope", "x", false);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("no finding with id"));
    }

    #[test]
    fn accept_overwrites_prior_entry() {
        let tmp = TempDir::new().unwrap();
        let paths = HealPaths::new(tmp.path());
        paths.ensure().unwrap();
        let f = finding("alpha", Severity::High);
        let f_id = f.id.clone();
        seed_record(&paths, vec![f]);

        run_accept(tmp.path(), &f_id, "first", false).unwrap();
        run_accept(tmp.path(), &f_id, "second", false).unwrap();

        let map = read_accepted(&paths.findings_accepted()).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map[&f_id].reason, "second");
    }
}
