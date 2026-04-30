//! `heal cache *` — read-only views of `.heal/checks/`, plus the
//! single mutating sub-command `mark-fixed` used by `/heal-fix` to
//! claim a commit as resolving a finding.
//!
//! Sub-commands:
//! - `cache log`        — newest-first list of `CheckRecord`s
//! - `cache show`       — detailed render of one record (unstable
//!   view; use `--json` for the stable shape)
//! - `cache diff`       — bucket findings into Resolved / Regressed /
//!   Improved / New / Unchanged across two records. With `--worktree`,
//!   compares the live tree to the latest cached record without
//!   writing anything.
//! - `cache mark-fixed` — append a `FixedFinding` line to
//!   `.heal/checks/fixed.jsonl` (the only `cache` command that writes).

use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::Serialize;

use crate::cli::CacheAction;
use crate::commands::check::{render, Filters};
use crate::core::calibration::Calibration;
use crate::core::check_cache::{
    append_fixed, config_hash_from_paths, iter_records, CheckRecord, FixedFinding,
};
use crate::core::config::load_from_project;
use crate::core::finding::Finding;
use crate::core::severity::Severity;
use crate::core::snapshot::{ansi_wrap, ANSI_CYAN, ANSI_GREEN, ANSI_RED, ANSI_YELLOW};
use crate::core::HealPaths;
use crate::observer::git;

pub fn run(project: &Path, action: CacheAction) -> Result<()> {
    let paths = HealPaths::new(project);
    match action {
        CacheAction::Log { json, limit } => run_log(&paths, json, limit),
        CacheAction::Show { check_id, json } => run_show(&paths, &check_id, json),
        CacheAction::Diff {
            from,
            to,
            worktree,
            all,
            json,
        } => run_diff(
            project,
            &paths,
            from.as_deref(),
            to.as_deref(),
            worktree,
            all,
            json,
        ),
        CacheAction::MarkFixed {
            finding_id,
            commit_sha,
        } => run_mark_fixed(&paths, &finding_id, &commit_sha),
    }
}

fn run_mark_fixed(paths: &HealPaths, finding_id: &str, commit_sha: &str) -> Result<()> {
    let entry = FixedFinding {
        finding_id: finding_id.to_owned(),
        commit_sha: commit_sha.to_owned(),
        fixed_at: Utc::now(),
    };
    append_fixed(&paths.checks_fixed_log(), &entry)?;
    println!(
        "marked {finding_id} as fixed by {commit_sha} (logged to {})",
        paths.checks_fixed_log().display(),
    );
    Ok(())
}

fn run_log(paths: &HealPaths, as_json: bool, limit: Option<usize>) -> Result<()> {
    let mut records = iter_records(&paths.checks_dir())?;
    if let Some(n) = limit {
        records.truncate(n);
    }
    if as_json {
        let payload: Vec<LogEntry> = records.iter().map(|(_, r)| LogEntry::from(r)).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).expect("LogEntry serialization is infallible")
        );
        return Ok(());
    }
    let mut stdout = std::io::stdout();
    let colorize = stdout.is_terminal();
    if records.is_empty() {
        writeln!(
            stdout,
            "no cache yet at {} — run `heal check`",
            paths.checks_dir().display(),
        )?;
        return Ok(());
    }
    for (_, rec) in &records {
        let counts = &rec.severity_counts;
        writeln!(
            stdout,
            "{}  {}  head={}  findings={}  {} {}  {} {}  {} {}",
            rec.check_id,
            rec.started_at.format("%Y-%m-%d %H:%M"),
            rec.head_sha.as_deref().unwrap_or("∅"),
            rec.findings.len(),
            ansi_wrap(ANSI_RED, "C", colorize),
            counts.critical,
            ansi_wrap(ANSI_YELLOW, "H", colorize),
            counts.high,
            ansi_wrap(ANSI_CYAN, "M", colorize),
            counts.medium,
        )?;
    }
    Ok(())
}

fn run_show(paths: &HealPaths, check_id: &str, as_json: bool) -> Result<()> {
    let records = iter_records(&paths.checks_dir())?;
    let record = records
        .into_iter()
        .find(|(_, r)| r.check_id == check_id)
        .map(|(_, r)| r)
        .ok_or_else(|| anyhow::anyhow!("no CheckRecord with check_id={check_id}"))?;
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&record).expect("CheckRecord serialization is infallible")
        );
        return Ok(());
    }
    eprintln!(
        "warning: `heal cache show` rendering is unstable; use `--json` for a stable contract.",
    );
    let mut stdout = std::io::stdout();
    let colorize = stdout.is_terminal();
    // Show full detail — turn on `--all` semantics so Medium/Ok aren't hidden.
    let filters = Filters {
        all: true,
        ..Filters::default()
    };
    render(&record, &[], &filters, colorize, &mut stdout)?;
    Ok(())
}

fn run_diff(
    project: &Path,
    paths: &HealPaths,
    from: Option<&str>,
    to: Option<&str>,
    worktree: bool,
    show_all: bool,
    as_json: bool,
) -> Result<()> {
    let records = iter_records(&paths.checks_dir())?;
    if records.is_empty() {
        bail!(
            "no cache yet at {} — run `heal check` first",
            paths.checks_dir().display()
        );
    }

    let to_record = if worktree {
        // Live scan, never written to disk. Compares the in-flight
        // working-tree state against the most recent cached run.
        let cfg = load_from_project(project).with_context(|| {
            format!(
                "loading {} (run `heal init` first?)",
                paths.config().display(),
            )
        })?;
        let calibration = Calibration::load(&paths.calibration())
            .ok()
            .map(|c| c.with_overrides(&cfg));
        let head_sha = git::head_sha(project);
        let worktree_clean = git::worktree_clean(project).unwrap_or(false);
        let config_hash = config_hash_from_paths(&paths.config(), &paths.calibration());
        crate::commands::check::build_fresh_record(
            project,
            &cfg,
            calibration.as_ref(),
            head_sha,
            worktree_clean,
            config_hash,
        )
    } else if let Some(id) = to {
        records
            .iter()
            .find(|(_, r)| r.check_id == id)
            .map(|(_, r)| r.clone())
            .ok_or_else(|| anyhow::anyhow!("no CheckRecord with check_id={id} (TO)"))?
    } else {
        records[0].1.clone()
    };

    // For `from`: explicit ULID, otherwise the latest cached record
    // that isn't `to_record` (i.e. records[0] in worktree mode,
    // records[1] otherwise).
    let from_record = if let Some(id) = from {
        records
            .iter()
            .find(|(_, r)| r.check_id == id)
            .map(|(_, r)| r.clone())
            .ok_or_else(|| anyhow::anyhow!("no CheckRecord with check_id={id} (FROM)"))?
    } else {
        let skip_id = (!worktree && to.is_none()).then(|| to_record.check_id.clone());
        records
            .iter()
            .map(|(_, r)| r.clone())
            .find(|r| skip_id.as_deref() != Some(r.check_id.as_str()))
            .ok_or_else(|| {
                anyhow::anyhow!("need at least two CheckRecords to diff — run `heal check` again")
            })?
    };

    let diff = compute_diff(&from_record, &to_record);

    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&diff).expect("CacheDiff serialization is infallible")
        );
        return Ok(());
    }

    let mut stdout = std::io::stdout();
    let colorize = stdout.is_terminal();
    render_diff(
        &from_record,
        &to_record,
        &diff,
        show_all,
        colorize,
        &mut stdout,
    )?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct LogEntry {
    check_id: String,
    started_at: chrono::DateTime<chrono::Utc>,
    head_sha: Option<String>,
    findings_count: usize,
    severity_counts: crate::core::snapshot::SeverityCounts,
    worktree_clean: bool,
}

impl From<&CheckRecord> for LogEntry {
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

#[derive(Debug, Clone, Serialize, Default)]
pub(super) struct CacheDiff {
    pub resolved: Vec<DiffEntry>,
    pub regressed: Vec<DiffEntry>,
    pub improved: Vec<DiffEntry>,
    pub new_findings: Vec<DiffEntry>,
    pub unchanged: Vec<DiffEntry>,
    /// `resolved.len() / from.findings.len()` as a `[0.0, 1.0]` ratio.
    /// Picked so the math matches the TODO mock: "3 resolved / 12 total
    /// → 25%". `total` here is the prior-run finding count.
    pub progress_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct DiffEntry {
    pub finding_id: String,
    pub metric: String,
    pub file: String,
    /// Prior Severity (None for `new_findings`).
    pub from_severity: Option<Severity>,
    /// Current Severity (None for `resolved`).
    pub to_severity: Option<Severity>,
    pub hotspot: bool,
}

fn compute_diff(from: &CheckRecord, to: &CheckRecord) -> CacheDiff {
    let from_by_id: HashMap<&str, &Finding> =
        from.findings.iter().map(|f| (f.id.as_str(), f)).collect();
    let to_by_id: HashMap<&str, &Finding> =
        to.findings.iter().map(|f| (f.id.as_str(), f)).collect();

    let mut diff = CacheDiff::default();

    for (id, prev) in &from_by_id {
        match to_by_id.get(id) {
            None => diff.resolved.push(DiffEntry::from_pair(prev, None)),
            Some(curr) if curr.severity > prev.severity => {
                diff.regressed.push(DiffEntry::from_pair(prev, Some(curr)));
            }
            Some(curr) if curr.severity < prev.severity => {
                diff.improved.push(DiffEntry::from_pair(prev, Some(curr)));
            }
            Some(curr) => diff.unchanged.push(DiffEntry::from_pair(prev, Some(curr))),
        }
    }
    for (id, curr) in &to_by_id {
        if !from_by_id.contains_key(id) {
            diff.new_findings.push(DiffEntry::from_new(curr));
        }
    }

    let total = from.findings.len();
    diff.progress_pct = if total == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        let pct = diff.resolved.len() as f64 / total as f64;
        pct
    };
    diff
}

impl DiffEntry {
    fn from_pair(prev: &Finding, curr: Option<&Finding>) -> Self {
        Self {
            finding_id: prev.id.clone(),
            metric: prev.metric.clone(),
            file: prev.location.file.display().to_string(),
            from_severity: Some(prev.severity),
            to_severity: curr.map(|c| c.severity),
            hotspot: curr.map_or(prev.hotspot, |c| c.hotspot),
        }
    }
    fn from_new(curr: &Finding) -> Self {
        Self {
            finding_id: curr.id.clone(),
            metric: curr.metric.clone(),
            file: curr.location.file.display().to_string(),
            from_severity: None,
            to_severity: Some(curr.severity),
            hotspot: curr.hotspot,
        }
    }
}

fn render_diff(
    from: &CheckRecord,
    to: &CheckRecord,
    diff: &CacheDiff,
    show_all: bool,
    colorize: bool,
    out: &mut impl Write,
) -> Result<()> {
    let title = ansi_wrap(ANSI_CYAN, "── HEAL cache diff", colorize);
    let bar: String = "─".repeat(45);
    writeln!(out, "{title} {bar}")?;
    writeln!(
        out,
        "  from: {}  HEAD={}  ({} findings)",
        from.started_at.format("%Y-%m-%d %H:%M"),
        from.head_sha.as_deref().unwrap_or("∅"),
        from.findings.len(),
    )?;
    writeln!(
        out,
        "  to:   {}  HEAD={}  ({} findings)",
        to.started_at.format("%Y-%m-%d %H:%M"),
        to.head_sha.as_deref().unwrap_or("∅"),
        to.findings.len(),
    )?;
    writeln!(out)?;

    render_bucket("✅ Resolved", ANSI_GREEN, &diff.resolved, colorize, out)?;
    render_bucket("⚠️  Regressed", ANSI_RED, &diff.regressed, colorize, out)?;
    render_bucket("➕ New", ANSI_YELLOW, &diff.new_findings, colorize, out)?;
    if show_all {
        render_bucket("🟢 Improved", ANSI_GREEN, &diff.improved, colorize, out)?;
        render_bucket("▫️ Unchanged", ANSI_CYAN, &diff.unchanged, colorize, out)?;
    } else {
        let hidden = diff.improved.len() + diff.unchanged.len();
        if hidden > 0 {
            writeln!(
                out,
                "  [Improved + Unchanged: {hidden} hidden — pass --all]"
            )?;
        }
    }
    writeln!(out)?;
    let resolved = diff.resolved.len();
    let total = from.findings.len();
    if total == 0 {
        writeln!(out, "  Progress: n/a (prior run had no findings)")?;
    } else {
        writeln!(
            out,
            "  Progress: {} resolved / {} total → {:.0}% complete",
            resolved,
            total,
            diff.progress_pct * 100.0,
        )?;
    }
    let close: String = "─".repeat(60);
    writeln!(out, "{close}")?;
    Ok(())
}

fn render_bucket(
    label: &str,
    color: &str,
    items: &[DiffEntry],
    colorize: bool,
    out: &mut impl Write,
) -> std::io::Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    writeln!(
        out,
        "{} ({})",
        ansi_wrap(color, label, colorize),
        items.len()
    )?;
    for e in items {
        let arrow = match (e.from_severity, e.to_severity) {
            (Some(from), Some(to)) if from != to => format!("({from:?} → {to:?})"),
            (Some(from), Some(_)) => format!("({from:?})"),
            (Some(from), None) => format!("({from:?} → ✓)"),
            (None, Some(to)) => format!("(new {to:?})"),
            (None, None) => String::new(),
        };
        let hot = if e.hotspot { " 🔥" } else { "" };
        writeln!(out, "  {} {} {arrow}{hot}", e.metric, e.file)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::finding::{Finding, Location};
    use crate::core::severity::Severity;
    use std::path::PathBuf;

    fn finding(id_seed: &str, severity: Severity) -> Finding {
        let mut f = Finding::new(
            "ccn",
            Location {
                file: PathBuf::from(format!("src/{id_seed}.ts")),
                line: Some(1),
                symbol: Some(id_seed.to_owned()),
            },
            "CCN=10 fn (rust)".into(),
            id_seed,
        );
        f.severity = severity;
        f
    }

    fn record(findings: Vec<Finding>) -> CheckRecord {
        CheckRecord::new(Some("abc".into()), true, "h".into(), findings)
    }

    #[test]
    fn diff_buckets_resolved_regressed_new_unchanged() {
        let stay = finding("stay", Severity::High);
        let dropped = finding("dropped", Severity::Medium);
        let regressed_a = finding("hot", Severity::High);
        let regressed_b = {
            // Same id (same content_seed + location), higher severity.
            let mut f = finding("hot", Severity::Critical);
            f.severity = Severity::Critical;
            assert_eq!(f.id, regressed_a.id);
            f
        };
        let new_one = finding("new", Severity::High);

        let from = record(vec![dropped.clone(), regressed_a.clone(), stay.clone()]);
        let to = record(vec![regressed_b.clone(), stay.clone(), new_one.clone()]);

        let diff = compute_diff(&from, &to);

        assert_eq!(diff.resolved.len(), 1);
        assert_eq!(diff.resolved[0].file, "src/dropped.ts");
        assert_eq!(diff.regressed.len(), 1);
        assert_eq!(diff.regressed[0].file, "src/hot.ts");
        assert_eq!(diff.regressed[0].from_severity, Some(Severity::High));
        assert_eq!(diff.regressed[0].to_severity, Some(Severity::Critical));
        assert_eq!(diff.unchanged.len(), 1);
        assert_eq!(diff.unchanged[0].file, "src/stay.ts");
        assert_eq!(diff.new_findings.len(), 1);
        assert_eq!(diff.new_findings[0].file, "src/new.ts");
        assert!(diff.improved.is_empty());

        // 1 resolved out of 3 prior = 33%.
        assert!((diff.progress_pct - (1.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn diff_buckets_improved_when_severity_drops() {
        let mut prev = finding("calm", Severity::Critical);
        prev.severity = Severity::Critical;
        let mut curr = finding("calm", Severity::Medium);
        curr.severity = Severity::Medium;
        assert_eq!(prev.id, curr.id);
        let diff = compute_diff(&record(vec![prev]), &record(vec![curr]));
        assert_eq!(diff.improved.len(), 1);
        assert!(diff.regressed.is_empty());
        // Improved counts in `unchanged + improved` but not in resolved → 0%.
        assert!((diff.progress_pct - 0.0).abs() < 1e-9);
    }

    #[test]
    fn diff_progress_zero_when_prior_empty() {
        let diff = compute_diff(
            &record(Vec::new()),
            &record(vec![finding("only", Severity::High)]),
        );
        assert!((diff.progress_pct - 0.0).abs() < 1e-9);
        assert_eq!(diff.new_findings.len(), 1);
    }

    #[test]
    fn mark_fixed_appends_entry_with_supplied_metadata() {
        use crate::core::check_cache::read_fixed;
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let paths = HealPaths::new(tmp.path());
        paths.ensure().unwrap();

        run_mark_fixed(&paths, "ccn:src/a.rs:foo:abc", "deadbeef").unwrap();
        run_mark_fixed(&paths, "ccn:src/b.rs:bar:def", "cafebabe").unwrap();

        let entries = read_fixed(&paths.checks_fixed_log()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].finding_id, "ccn:src/a.rs:foo:abc");
        assert_eq!(entries[0].commit_sha, "deadbeef");
        assert_eq!(entries[1].finding_id, "ccn:src/b.rs:bar:def");
        assert_eq!(entries[1].commit_sha, "cafebabe");
    }
}
