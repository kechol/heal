//! `.heal/findings/accepted.json` — per-finding "won't fix /
//! acknowledged intrinsic" map. Distinct from `fixed.json`: accepted
//! entries persist across re-detections by design (the team has
//! decided the finding is intrinsic), where a re-detected `fixed`
//! entry would move to `regressed.jsonl` and clutter the audit trail.
//!
//! Decoration is applied at render time via [`decorate_findings`].
//! `latest.json` keeps raw observer truth; every renderer (status,
//! diff, post-commit nudge, JSON output) folds in the accepted map
//! just before emitting. The observer cache stays cheap to write and
//! `heal mark accept` takes effect without a rescan.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::error::{Error, Result};
use crate::core::finding::Finding;
use crate::core::severity::Severity;

/// One accepted finding. Snapshots the severity / hotspot / summary
/// at accept time so a later auditor can see what the decision was
/// made against — the live finding may have drifted by then.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AcceptedFinding {
    /// Free-form rationale. Empty allowed — the AI agent driving
    /// `heal mark accept` is expected to fill it; a strict CLI gate
    /// would just push friction onto the rare hand-invocation case.
    #[serde(default)]
    pub reason: String,
    pub file: String,
    pub metric: String,
    /// Drift detection (escalation) compares the *current*
    /// classification against this snapshot.
    pub severity: Severity,
    #[serde(default)]
    pub hotspot: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_value: Option<f64>,
    pub summary: String,
    pub accepted_at: DateTime<Utc>,
    /// `Name <email>` from git config at accept time. `None` when
    /// the config wasn't available (CI bot, detached env).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_by: Option<String>,
}

pub type AcceptedMap = BTreeMap<String, AcceptedFinding>;

/// Read `accepted.json`. Empty map on missing or corrupt — same
/// degrade-quietly contract as [`crate::core::findings_cache::read_fixed`]
/// so a broken file never blocks `heal status`.
pub fn read_accepted(path: &Path) -> Result<AcceptedMap> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(AcceptedMap::new()),
        Err(e) => {
            return Err(Error::Io {
                path: path.to_path_buf(),
                source: e,
            })
        }
    };
    match serde_json::from_slice::<AcceptedMap>(&bytes) {
        Ok(map) => Ok(map),
        Err(err) => {
            eprintln!(
                "heal: ignoring unreadable {} ({err}); the next mark accept will rewrite it",
                path.display(),
            );
            Ok(AcceptedMap::new())
        }
    }
}

/// Atomically rewrite `accepted.json`.
pub fn write_accepted(path: &Path, map: &AcceptedMap) -> Result<()> {
    let body = serde_json::to_vec_pretty(map).expect("AcceptedMap serialization is infallible");
    crate::core::fs::atomic_write(path, &body)
}

pub fn upsert_accepted(path: &Path, finding_id: &str, entry: AcceptedFinding) -> Result<()> {
    let mut map = read_accepted(path)?;
    map.insert(finding_id.to_owned(), entry);
    write_accepted(path, &map)
}

/// Remove an accept entry. Returns the entry that was removed (or
/// `None` when the id wasn't present) so callers can confirm what
/// was unmarked.
pub fn remove_accepted(path: &Path, finding_id: &str) -> Result<Option<AcceptedFinding>> {
    let mut map = read_accepted(path)?;
    let removed = map.remove(finding_id);
    if removed.is_some() {
        write_accepted(path, &map)?;
    }
    Ok(removed)
}

pub fn decorate_findings(findings: &mut [Finding], map: &AcceptedMap) {
    for f in findings.iter_mut() {
        f.accepted = map.contains_key(&f.id);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedDrift {
    pub finding_id: String,
    pub file: String,
    pub was: Severity,
    pub now: Severity,
}

/// Severity escalations only. File-deleted, not-detected, and
/// same-severity-different-value cases stay quiet — those surface in
/// `heal mark accept --list`, not as runtime warnings (severity is
/// HEAL's only decision boundary; raw metric values are an
/// implementation detail of the classifier).
#[must_use]
pub fn reconcile_accepted(map: &AcceptedMap, findings: &[Finding]) -> Vec<AcceptedDrift> {
    let mut out = Vec::new();
    for f in findings {
        let Some(entry) = map.get(&f.id) else {
            continue;
        };
        if f.severity > entry.severity {
            out.push(AcceptedDrift {
                finding_id: f.id.clone(),
                file: entry.file.clone(),
                was: entry.severity,
                now: f.severity,
            });
        }
    }
    out
}

/// Build an `AcceptedFinding` from a live `Finding`. Used by the
/// `heal mark accept` command path so the snapshot fields stay in
/// sync with the finding the user is accepting.
#[must_use]
pub fn snapshot(
    finding: &Finding,
    reason: String,
    accepted_at: DateTime<Utc>,
    accepted_by: Option<String>,
) -> AcceptedFinding {
    AcceptedFinding {
        reason,
        file: finding.location.file.to_string_lossy().into_owned(),
        metric: finding.metric.clone(),
        severity: finding.severity,
        hotspot: finding.hotspot,
        metric_value: finding.metric_value(),
        summary: finding.summary.clone(),
        accepted_at,
        accepted_by,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::finding::{Finding, Location};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn finding(metric: &str, file: &str, severity: Severity, summary: &str) -> Finding {
        let mut f = Finding::new(
            metric,
            Location::file(PathBuf::from(file)),
            summary.to_owned(),
            "seed",
        );
        f.severity = severity;
        f
    }

    fn accepted(metric: &str, severity: Severity, summary: &str) -> AcceptedFinding {
        AcceptedFinding {
            reason: "intrinsic dispatcher".into(),
            file: "src/foo.ts".into(),
            metric: metric.to_owned(),
            severity,
            hotspot: false,
            metric_value: None,
            summary: summary.to_owned(),
            accepted_at: Utc::now(),
            accepted_by: None,
        }
    }

    #[test]
    fn round_trips_through_disk() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("accepted.json");
        let entry = accepted("ccn", Severity::Critical, "CCN=28 foo");
        upsert_accepted(&path, "id-1", entry.clone()).unwrap();
        let back = read_accepted(&path).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back.get("id-1"), Some(&entry));
    }

    #[test]
    fn read_missing_returns_empty_map() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nonexistent.json");
        let map = read_accepted(&path).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn read_corrupt_logs_and_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("accepted.json");
        std::fs::write(&path, b"not json").unwrap();
        let map = read_accepted(&path).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn upsert_overwrites_existing_entry() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("accepted.json");
        upsert_accepted(&path, "id-1", accepted("ccn", Severity::High, "CCN=12 foo")).unwrap();
        upsert_accepted(
            &path,
            "id-1",
            accepted("ccn", Severity::Critical, "CCN=28 foo"),
        )
        .unwrap();
        let back = read_accepted(&path).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back.get("id-1").unwrap().severity, Severity::Critical);
    }

    #[test]
    fn remove_returns_entry_and_drops_it() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("accepted.json");
        let entry = accepted("ccn", Severity::High, "CCN=12 foo");
        upsert_accepted(&path, "id-1", entry.clone()).unwrap();
        let removed = remove_accepted(&path, "id-1").unwrap();
        assert_eq!(removed.as_ref(), Some(&entry));
        let back = read_accepted(&path).unwrap();
        assert!(back.is_empty());
    }

    #[test]
    fn remove_unknown_id_is_noop() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("accepted.json");
        let removed = remove_accepted(&path, "nope").unwrap();
        assert!(removed.is_none());
    }

    #[test]
    fn deny_unknown_fields() {
        let raw = r#"{
            "id-1": {
                "reason": "x",
                "file": "src/foo.ts",
                "metric": "ccn",
                "severity": "high",
                "summary": "CCN=12 foo",
                "accepted_at": "2026-05-03T12:00:00Z",
                "extra_field": "rejected"
            }
        }"#;
        let res = serde_json::from_str::<AcceptedMap>(raw);
        assert!(res.is_err(), "deny_unknown_fields should reject extras");
    }

    #[test]
    fn decorate_findings_sets_accepted_flag_by_id() {
        let mut findings = vec![
            finding("ccn", "src/a.ts", Severity::High, "CCN=12"),
            finding("ccn", "src/b.ts", Severity::Critical, "CCN=28"),
        ];
        let mut map = AcceptedMap::new();
        map.insert(
            findings[0].id.clone(),
            accepted("ccn", Severity::High, "CCN=12"),
        );
        decorate_findings(&mut findings, &map);
        assert!(findings[0].accepted);
        assert!(!findings[1].accepted);
    }

    #[test]
    fn reconcile_returns_escalations_only() {
        let f_high_to_crit = finding("ccn", "src/a.ts", Severity::Critical, "CCN=28");
        let f_high_stable = finding("ccn", "src/b.ts", Severity::High, "CCN=12");
        let f_high_improved = finding("ccn", "src/c.ts", Severity::Medium, "CCN=8");
        let mut map = AcceptedMap::new();
        map.insert(
            f_high_to_crit.id.clone(),
            accepted("ccn", Severity::High, "CCN=12"),
        );
        map.insert(
            f_high_stable.id.clone(),
            accepted("ccn", Severity::High, "CCN=12"),
        );
        map.insert(
            f_high_improved.id.clone(),
            accepted("ccn", Severity::High, "CCN=12"),
        );
        let drifts = reconcile_accepted(
            &map,
            &[f_high_to_crit.clone(), f_high_stable, f_high_improved],
        );
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].finding_id, f_high_to_crit.id);
        assert_eq!(drifts[0].was, Severity::High);
        assert_eq!(drifts[0].now, Severity::Critical);
    }

    #[test]
    fn reconcile_quiet_on_undetected_findings() {
        // Accepted entry exists but the corresponding finding is no
        // longer in the current scan. Quiet — surfaced via `heal
        // accepted list`, not a status-time warning.
        let mut map = AcceptedMap::new();
        map.insert(
            "vanished-id".into(),
            accepted("ccn", Severity::High, "CCN=12"),
        );
        let drifts = reconcile_accepted(&map, &[]);
        assert!(drifts.is_empty());
    }

    #[test]
    fn snapshot_extracts_ccn_value() {
        let f = finding(
            "ccn",
            "src/foo.ts",
            Severity::Critical,
            "CCN=28 processOrder",
        );
        let entry = snapshot(&f, "intrinsic".into(), Utc::now(), None);
        assert_eq!(entry.metric_value, Some(28.0));
    }

    #[test]
    fn snapshot_no_value_for_label_metrics() {
        let f = finding(
            "duplication",
            "src/foo.ts",
            Severity::Critical,
            "Duplicated block (3 sites)",
        );
        let entry = snapshot(&f, "intrinsic".into(), Utc::now(), None);
        assert_eq!(entry.metric_value, None);
    }
}
