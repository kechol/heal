//! `.heal/checks/` — the result cache for `heal status`.
//!
//! `heal status` is the **single writer** of `<root>/.heal/checks/YYYY-MM.jsonl`
//! and `latest.json`: every run produces a [`CheckRecord`] which is
//! appended to the segment and mirrored atomically into `latest.json`.
//! `heal checks` and `heal diff` are pure readers. The only other
//! writer is `heal mark-fixed`, which appends to `fixed.jsonl`.
//!
//! The cache models a TODO list: each `Finding.id` is decision-stable
//! (see `crate::core::finding`), so the *same* unfixed problem keeps the
//! same id across consecutive `heal status` runs. Skills can diff two
//! records to see what's been resolved, what's new, and what's
//! regressed.
//!
//! Three side files live next to the JSONL stream:
//!   - `latest.json`     — full mirror of the most recent record
//!   - `fixed.jsonl`     — append-only "skill committed a fix" markers
//!   - `regressed.jsonl` — append-only "fix re-detected" markers
//!
//! ## Idempotency contract
//!
//! [`is_fresh_against`] returns true when `(head_sha, config_hash,
//! worktree_clean)` matches the supplied baseline — `heal status` short-
//! circuits on a fresh cache and reuses the latest record. Dirty
//! worktrees never count as fresh (any untracked file invalidates the
//! cache; we cannot trust the on-disk numbers).
//!
//! ## fixed.jsonl reconciliation
//!
//! When a skill commits a fix, it appends a [`FixedFinding`] to
//! `fixed.jsonl`. On the next `heal status`, [`reconcile_fixed`] walks
//! the new findings:
//!   - if a fixed `finding_id` is **not** present in the new record →
//!     the entry remains marked fixed.
//!   - if a fixed `finding_id` **is** present → the entry is removed
//!     from `fixed.jsonl` and a [`RegressedEntry`] is appended to
//!     `regressed.jsonl` so the renderer can warn the user.

use std::collections::HashSet;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::error::{Error, Result};
use crate::core::eventlog::{Event, EventLog};
use crate::core::finding::Finding;
use crate::core::hash::{fnv1a_64_chunked, fnv1a_hex};
use crate::core::snapshot::SeverityCounts;

/// Stable schema version for [`CheckRecord`]. Bump on breaking field
/// renames so the reader can skip records it can't decode rather than
/// failing the whole stream.
pub const CHECK_RECORD_VERSION: u32 = 1;

/// One execution of `heal status`. The unit of read in the cache:
/// `heal checks` enumerates records and `heal diff <git-ref>` looks
/// one up by `head_sha` to bucket-diff against the live worktree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckRecord {
    pub version: u32,
    /// Crockford-base32 ULID. The leading 48 bits are the millisecond
    /// timestamp, so lexicographic order = chronological order.
    pub check_id: String,
    pub started_at: DateTime<Utc>,
    /// `None` when `heal status` ran outside a git repo or HEAD is
    /// unborn. The cache still records the run; the freshness check
    /// just won't match any future invocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    pub worktree_clean: bool,
    /// Hex digest of `(config.toml || calibration.toml)`. Two runs at
    /// the same `head_sha` but different configs / calibrations produce
    /// different hashes and thus distinct records.
    pub config_hash: String,
    pub severity_counts: SeverityCounts,
    /// Per-workspace tally when `[[project.workspaces]]` is declared,
    /// keyed by workspace path (the same string as `Finding.workspace`).
    /// Empty when no workspaces are configured. Findings outside every
    /// declared workspace are not counted here — they only appear in
    /// the top-level `severity_counts`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<WorkspaceSummary>,
    pub findings: Vec<Finding>,
}

/// Per-workspace severity tally, ordered the same as
/// `[[project.workspaces]]` entries appear in `config.toml`. Lives on
/// `CheckRecord` so skills don't have to re-derive it from
/// `findings[].workspace`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceSummary {
    pub path: String,
    pub severity_counts: SeverityCounts,
}

impl CheckRecord {
    /// Build a record from the freshly classified findings. The id is
    /// generated from the current wall clock; if you need a
    /// deterministic id (tests), assign `check_id` after construction.
    #[must_use]
    pub fn new(
        head_sha: Option<String>,
        worktree_clean: bool,
        config_hash: String,
        findings: Vec<Finding>,
    ) -> Self {
        let started_at = Utc::now();
        let severity_counts = tally_findings(&findings);
        let workspaces = workspace_summaries(&findings);
        Self {
            version: CHECK_RECORD_VERSION,
            check_id: ulid::Ulid::new().to_string(),
            started_at,
            head_sha,
            worktree_clean,
            config_hash,
            severity_counts,
            workspaces,
            findings,
        }
    }

    /// Return a copy with `findings` and `severity_counts` narrowed to
    /// `workspace`. `workspaces` keeps only the matching summary (or is
    /// empty if `workspace` matches nothing). Callers use this to make
    /// `heal status --json --workspace …` mirror the filtered console
    /// view without forcing skills to re-aggregate the full record.
    #[must_use]
    pub fn project_to_workspace(&self, workspace: &str) -> Self {
        let findings: Vec<Finding> = self
            .findings
            .iter()
            .filter(|f| f.workspace.as_deref() == Some(workspace))
            .cloned()
            .collect();
        let severity_counts = tally_findings(&findings);
        let workspaces = self
            .workspaces
            .iter()
            .filter(|w| w.path == workspace)
            .cloned()
            .collect();
        Self {
            version: self.version,
            check_id: self.check_id.clone(),
            started_at: self.started_at,
            head_sha: self.head_sha.clone(),
            worktree_clean: self.worktree_clean,
            config_hash: self.config_hash.clone(),
            severity_counts,
            workspaces,
            findings,
        }
    }

    /// True iff `(head_sha, config_hash, worktree_clean)` matches and
    /// the worktree is clean — a dirty tree is never fresh because the
    /// recorded numbers don't reflect the on-disk source.
    #[must_use]
    pub fn is_fresh_against(
        &self,
        head_sha: Option<&str>,
        config_hash: &str,
        worktree_clean: bool,
    ) -> bool {
        if !self.worktree_clean || !worktree_clean {
            return false;
        }
        self.head_sha.as_deref() == head_sha && self.config_hash == config_hash
    }
}

fn tally_findings(findings: &[Finding]) -> SeverityCounts {
    let mut counts = SeverityCounts::default();
    for f in findings {
        counts.tally(f.severity);
    }
    counts
}

/// Group findings by `Finding.workspace`, drop the unwined bucket, and
/// produce one [`WorkspaceSummary`] per declared workspace. Output is
/// alphabetic by path so `heal status --json` is reproducible. Used by
/// [`CheckRecord::new`].
fn workspace_summaries(findings: &[Finding]) -> Vec<WorkspaceSummary> {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<String, SeverityCounts> = BTreeMap::new();
    for f in findings {
        let Some(ws) = f.workspace.as_deref() else {
            continue;
        };
        groups.entry(ws.to_owned()).or_default().tally(f.severity);
    }
    groups
        .into_iter()
        .map(|(path, severity_counts)| WorkspaceSummary {
            path,
            severity_counts,
        })
        .collect()
}

/// "A skill committed a fix that resolves this finding". The skill (or
/// equivalent caller) appends one of these via [`append_fixed`] when it
/// lands a commit; the next `heal status` reconciles fixed.jsonl against
/// the new findings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FixedFinding {
    pub finding_id: String,
    pub commit_sha: String,
    pub fixed_at: DateTime<Utc>,
}

/// A previously-fixed finding that was re-detected by a later
/// `heal status`. Either the skill's commit didn't fully address the
/// problem or a separate commit reintroduced it. Surfaced in the renderer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegressedEntry {
    pub finding_id: String,
    pub previous_commit_sha: String,
    pub previous_fixed_at: DateTime<Utc>,
    pub regressed_check_id: String,
    pub regressed_at: DateTime<Utc>,
}

/// FNV-1a 64-bit digest of the concatenated config bytes, as 16-hex.
/// Stable across processes and Rust toolchains (CLAUDE.md §Hashing)
/// so a `rustc` upgrade can't silently invalidate every recorded
/// `config_hash`.
#[must_use]
pub fn config_hash(config_toml: &[u8], calibration_toml: &[u8]) -> String {
    fnv1a_hex(fnv1a_64_chunked(&[config_toml, calibration_toml]))
}

/// Read `config.toml` and `calibration.toml` (best-effort) and hash the
/// pair. Missing files contribute the empty byte slice so a fresh
/// project still produces a stable hash.
#[must_use]
pub fn config_hash_from_paths(config: &Path, calibration: &Path) -> String {
    let cfg = std::fs::read(config).unwrap_or_default();
    let cal = std::fs::read(calibration).unwrap_or_default();
    config_hash(&cfg, &cal)
}

/// Append `record` to `<dir>/YYYY-MM.jsonl` and atomically refresh
/// `<dir>/latest.json`. Both writes share the same JSON shape, so a
/// reader that lost the JSONL stream can still reconstruct from
/// `latest.json` (or vice versa for the most recent record).
pub fn write_record(checks_dir: &Path, latest_path: &Path, record: &CheckRecord) -> Result<()> {
    EventLog::new(checks_dir).append(&Event {
        timestamp: record.started_at,
        event: "check".to_owned(),
        data: serde_json::to_value(record).expect("CheckRecord serialization is infallible"),
    })?;
    let body = serde_json::to_vec_pretty(record).expect("CheckRecord serialization is infallible");
    crate::core::fs::atomic_write(latest_path, &body)
}

/// Read the most recently written `CheckRecord` via `latest.json`.
/// Returns `Ok(None)` when the file doesn't exist (fresh project) or
/// when the record's `version` is from a future build this binary can't
/// safely interpret — letting an older binary degrade to "no cache" is
/// preferable to misreading a newer schema.
pub fn read_latest(latest_path: &Path) -> Result<Option<CheckRecord>> {
    let bytes = match std::fs::read(latest_path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(Error::Io {
                path: latest_path.to_path_buf(),
                source: e,
            })
        }
    };
    let record: CheckRecord = serde_json::from_slice(&bytes).map_err(|e| Error::CacheParse {
        path: latest_path.to_path_buf(),
        source: e,
    })?;
    if record.version > CHECK_RECORD_VERSION {
        return Ok(None);
    }
    Ok(Some(record))
}

/// Walk `checks/YYYY-MM.jsonl` segments newest-first, decoding events
/// whose `event` is `"check"`. Records that fail to decode (legacy /
/// truncated) are skipped silently — same contract as
/// `MetricsSnapshot::latest_in_segments`.
pub fn iter_records(checks_dir: &Path) -> Result<Vec<(DateTime<Utc>, CheckRecord)>> {
    let segments = EventLog::new(checks_dir).segments()?;
    let mut out: Vec<(DateTime<Utc>, CheckRecord)> = Vec::new();
    for ev in EventLog::iter_segments(segments).flatten() {
        if ev.event != "check" {
            continue;
        }
        if let Ok(rec) = serde_json::from_value::<CheckRecord>(ev.data.clone()) {
            if rec.version <= CHECK_RECORD_VERSION {
                out.push((ev.timestamp, rec));
            }
        }
    }
    out.sort_by_key(|(ts, _)| *ts);
    out.reverse();
    Ok(out)
}

/// Locate a record in a previously-loaded list by `check_id`. Pure
/// linear search — the cache is small enough (one record per `heal
/// check --refresh`) that an index isn't worth the bookkeeping.
#[must_use]
pub fn find_by_id<'a>(
    records: &'a [(DateTime<Utc>, CheckRecord)],
    check_id: &str,
) -> Option<&'a CheckRecord> {
    records
        .iter()
        .find(|(_, r)| r.check_id == check_id)
        .map(|(_, r)| r)
}

/// Lightweight projection of [`CheckRecord`] used by `heal checks`
/// (top-level browser) and any caller that wants the index fields
/// without the embedded `findings` vector. Construct via `From<&CheckRecord>`.
#[derive(Debug, Clone, Serialize)]
pub struct CheckRecordSummary {
    pub check_id: String,
    pub started_at: DateTime<Utc>,
    pub head_sha: Option<String>,
    pub findings_count: usize,
    pub severity_counts: SeverityCounts,
    pub worktree_clean: bool,
}

impl From<&CheckRecord> for CheckRecordSummary {
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

/// Append one entry to `fixed.jsonl`. Each line is one JSON-encoded
/// [`FixedFinding`].
pub fn append_fixed(fixed_log: &Path, entry: &FixedFinding) -> Result<()> {
    append_jsonl(fixed_log, entry)
}

/// Read the full `fixed.jsonl` history. Lines that fail to decode are
/// skipped silently to keep the cache forward-compatible across schema
/// additions.
pub fn read_fixed(fixed_log: &Path) -> Result<Vec<FixedFinding>> {
    read_jsonl(fixed_log)
}

/// Read the full `regressed.jsonl` history.
pub fn read_regressed(regressed_log: &Path) -> Result<Vec<RegressedEntry>> {
    read_jsonl(regressed_log)
}

/// Reconcile `fixed.jsonl` against the findings in `record`:
///   - any fixed `finding_id` re-detected in `record` is **removed**
///     from fixed.jsonl
///   - and appended to `regressed.jsonl` as a [`RegressedEntry`]
///
/// Returns the regressed entries so the caller (the renderer) can warn
/// the user. Fixed entries that are *not* re-detected stay in
/// `fixed.jsonl` untouched.
///
/// Both writes use temp-file + rename so a SIGINT mid-reconcile can't
/// leave a half-rewritten `fixed.jsonl`.
pub fn reconcile_fixed(
    fixed_log: &Path,
    regressed_log: &Path,
    record: &CheckRecord,
) -> Result<Vec<RegressedEntry>> {
    let fixed = read_fixed(fixed_log)?;
    if fixed.is_empty() {
        return Ok(Vec::new());
    }
    let active_ids: HashSet<&str> = record.findings.iter().map(|f| f.id.as_str()).collect();

    let mut surviving: Vec<FixedFinding> = Vec::with_capacity(fixed.len());
    let mut regressed: Vec<RegressedEntry> = Vec::new();
    for entry in fixed {
        if active_ids.contains(entry.finding_id.as_str()) {
            regressed.push(RegressedEntry {
                finding_id: entry.finding_id.clone(),
                previous_commit_sha: entry.commit_sha.clone(),
                previous_fixed_at: entry.fixed_at,
                regressed_check_id: record.check_id.clone(),
                regressed_at: record.started_at,
            });
        } else {
            surviving.push(entry);
        }
    }

    if regressed.is_empty() {
        return Ok(Vec::new());
    }

    rewrite_jsonl(fixed_log, &surviving)?;
    for entry in &regressed {
        append_jsonl(regressed_log, entry)?;
    }
    Ok(regressed)
}

fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    use std::io::Write as _;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::Io {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }
    let line = serde_json::to_string(value).expect("entry serialization is infallible");
    let mut body = line.into_bytes();
    body.push(b'\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| Error::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
    file.write_all(&body).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(())
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(Error::Io {
                path: path.to_path_buf(),
                source: e,
            })
        }
    };
    let mut out = Vec::new();
    for line in bytes.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_slice::<T>(line) {
            out.push(value);
        }
    }
    Ok(out)
}

fn rewrite_jsonl<T: Serialize>(path: &Path, values: &[T]) -> Result<()> {
    let mut body: Vec<u8> = Vec::new();
    for v in values {
        let line = serde_json::to_string(v).expect("entry serialization is infallible");
        body.extend_from_slice(line.as_bytes());
        body.push(b'\n');
    }
    crate::core::fs::atomic_write(path, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::finding::{Finding, Location};
    use crate::core::severity::Severity;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn finding(id_seed: &str, severity: Severity) -> Finding {
        let mut f = Finding::new(
            "ccn",
            Location {
                file: PathBuf::from(format!("src/{id_seed}.rs")),
                line: Some(1),
                symbol: Some(id_seed.to_owned()),
            },
            format!("CCN finding {id_seed}"),
            id_seed,
        );
        f.severity = severity;
        f
    }

    #[test]
    fn check_id_is_unique_across_calls() {
        let r1 = CheckRecord::new(None, false, "h".into(), Vec::new());
        let r2 = CheckRecord::new(None, false, "h".into(), Vec::new());
        assert_ne!(r1.check_id, r2.check_id);
    }

    #[test]
    fn project_to_workspace_narrows_findings_summary_and_workspaces() {
        let mut a = finding("alpha", Severity::High);
        a.workspace = Some("packages/web".into());
        let mut b = finding("beta", Severity::Critical);
        b.workspace = Some("packages/api".into());
        let c = finding("gamma", Severity::Medium); // unscoped
        let rec = CheckRecord::new(
            Some("sha".into()),
            true,
            "h".into(),
            vec![a.clone(), b.clone(), c.clone()],
        );
        let web = rec.project_to_workspace("packages/web");
        assert_eq!(web.findings.len(), 1);
        assert_eq!(web.findings[0].id, a.id);
        assert_eq!(web.workspaces.len(), 1);
        assert_eq!(web.workspaces[0].path, "packages/web");
        assert_eq!(web.severity_counts.high, 1);
        assert_eq!(web.severity_counts.critical, 0);
        // identity bits preserved.
        assert_eq!(web.head_sha, rec.head_sha);
        assert_eq!(web.config_hash, rec.config_hash);
        assert_eq!(web.check_id, rec.check_id);
    }

    #[test]
    fn config_hash_distinguishes_concatenation_boundary() {
        // Without the field separator, ("ab", "c") and ("a", "bc") would
        // collide. Verify they don't.
        let a = config_hash(b"ab", b"c");
        let b = config_hash(b"a", b"bc");
        assert_ne!(a, b);
    }

    #[test]
    fn config_hash_is_stable_across_calls() {
        let a = config_hash(b"foo", b"bar");
        let b = config_hash(b"foo", b"bar");
        assert_eq!(a, b);
    }

    #[test]
    fn write_then_read_round_trips() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("checks");
        let latest = dir.join("latest.json");
        let rec = CheckRecord::new(
            Some("abc".into()),
            true,
            "deadbeef".into(),
            vec![finding("foo", Severity::Critical)],
        );
        write_record(&dir, &latest, &rec).unwrap();
        let back = read_latest(&latest).unwrap().expect("record present");
        assert_eq!(back.check_id, rec.check_id);
        assert_eq!(back.findings.len(), 1);
        assert_eq!(back.severity_counts.critical, 1);
    }

    #[test]
    fn iter_records_returns_newest_first() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("checks");
        let latest = dir.join("latest.json");

        // Write two records back-to-back; ULIDs encode time so check_id
        // ordering is monotonic.
        let r1 = CheckRecord::new(Some("a".into()), true, "h".into(), Vec::new());
        write_record(&dir, &latest, &r1).unwrap();
        // Sleep a millisecond so the second record's started_at is strictly later.
        std::thread::sleep(std::time::Duration::from_millis(2));
        let r2 = CheckRecord::new(Some("b".into()), true, "h".into(), Vec::new());
        write_record(&dir, &latest, &r2).unwrap();

        let records = iter_records(&dir).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].1.check_id, r2.check_id);
        assert_eq!(records[1].1.check_id, r1.check_id);
    }

    #[test]
    fn freshness_requires_clean_worktree_on_both_sides() {
        let rec = CheckRecord::new(Some("a".into()), true, "h".into(), Vec::new());
        // Same head + config + clean → fresh.
        assert!(rec.is_fresh_against(Some("a"), "h", true));
        // Dirty current worktree → not fresh.
        assert!(!rec.is_fresh_against(Some("a"), "h", false));
        // Different config → not fresh.
        assert!(!rec.is_fresh_against(Some("a"), "h2", true));
        // Different head → not fresh.
        assert!(!rec.is_fresh_against(Some("b"), "h", true));

        // A previously-dirty record is never fresh, even if everything else matches.
        let dirty = CheckRecord::new(Some("a".into()), false, "h".into(), Vec::new());
        assert!(!dirty.is_fresh_against(Some("a"), "h", true));
    }

    #[test]
    fn reconcile_fixed_drops_redetected_and_records_regression() {
        let tmp = TempDir::new().unwrap();
        let fixed_log = tmp.path().join("fixed.jsonl");
        let regressed_log = tmp.path().join("regressed.jsonl");

        // Two prior fixes; one re-detected, one stays clean.
        let still_fixed = finding("clean", Severity::Critical);
        let regressed = finding("regressed", Severity::High);
        append_fixed(
            &fixed_log,
            &FixedFinding {
                finding_id: still_fixed.id.clone(),
                commit_sha: "sha-clean".into(),
                fixed_at: Utc::now(),
            },
        )
        .unwrap();
        append_fixed(
            &fixed_log,
            &FixedFinding {
                finding_id: regressed.id.clone(),
                commit_sha: "sha-regressed".into(),
                fixed_at: Utc::now(),
            },
        )
        .unwrap();

        // New CheckRecord re-detects only the regressed finding.
        let rec = CheckRecord::new(None, true, "h".into(), vec![regressed.clone()]);
        let surfaced = reconcile_fixed(&fixed_log, &regressed_log, &rec).unwrap();
        assert_eq!(surfaced.len(), 1);
        assert_eq!(surfaced[0].finding_id, regressed.id);

        // fixed.jsonl now contains only the still-fixed entry.
        let surviving = read_fixed(&fixed_log).unwrap();
        assert_eq!(surviving.len(), 1);
        assert_eq!(surviving[0].finding_id, still_fixed.id);

        // regressed.jsonl gained one entry.
        let regs = read_regressed(&regressed_log).unwrap();
        assert_eq!(regs.len(), 1);
        assert_eq!(regs[0].finding_id, regressed.id);
        assert_eq!(regs[0].regressed_check_id, rec.check_id);
    }

    #[test]
    fn reconcile_fixed_is_noop_when_nothing_regresses() {
        let tmp = TempDir::new().unwrap();
        let fixed_log = tmp.path().join("fixed.jsonl");
        let regressed_log = tmp.path().join("regressed.jsonl");

        let f = finding("only", Severity::Critical);
        append_fixed(
            &fixed_log,
            &FixedFinding {
                finding_id: f.id.clone(),
                commit_sha: "sha".into(),
                fixed_at: Utc::now(),
            },
        )
        .unwrap();

        // New record has no findings — nothing regressed.
        let rec = CheckRecord::new(None, true, "h".into(), Vec::new());
        let surfaced = reconcile_fixed(&fixed_log, &regressed_log, &rec).unwrap();
        assert!(surfaced.is_empty());
        assert_eq!(read_fixed(&fixed_log).unwrap().len(), 1);
        assert!(!regressed_log.exists());
    }
}
