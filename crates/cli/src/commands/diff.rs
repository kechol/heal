//! `heal diff [<git-ref>]` — bucket-style diff between the current
//! findings and a cached `CheckRecord` whose `head_sha` matches the
//! resolved git ref. Default ref: `HEAD` ("how does my live worktree
//! compare to the last commit?").
//!
//! The cache is the only source of historical state. If no `CheckRecord`
//! has been written for the ref's commit (e.g. you haven't run
//! `heal status` since checking out an older branch), the command
//! errors with a hint instead of silently checking out the ref to
//! re-scan.
//!
//! Output buckets — Resolved / Regressed / Improved / New / Unchanged —
//! plus a progress percentage. JSON shape is stable for skills and CI.

use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::core::check_cache::{iter_records, CheckRecord};
use crate::core::config::load_from_project;
use crate::core::finding::Finding;
use crate::core::severity::Severity;
use crate::core::snapshot::{ansi_wrap, ANSI_CYAN, ANSI_GREEN, ANSI_RED, ANSI_YELLOW};
use crate::core::HealPaths;
use crate::observer::git;

pub fn run(
    project: &Path,
    revspec: &str,
    workspace: Option<&str>,
    show_all: bool,
    as_json: bool,
) -> Result<()> {
    let paths = HealPaths::new(project);
    let target_sha = git::resolve_ref(project, revspec).ok_or_else(|| {
        anyhow::anyhow!(
            "could not resolve git ref `{revspec}` in {} — is this a git repo?",
            project.display(),
        )
    })?;

    let from_record = find_cached_record(&paths, &target_sha)?.ok_or_else(|| {
        anyhow::anyhow!(
            "no cached `heal status` record for {revspec} (sha {short}). \
             Either commit the work and run `heal status` so a record \
             exists, or check out {revspec} and run `heal status --refresh` first.",
            short = &target_sha[..target_sha.len().min(8)],
        )
    })?;
    let to_record = build_live_record(project, &paths)?;

    let diff = compute_diff(&from_record, &to_record, workspace);
    if as_json {
        super::emit_json(&DiffReport {
            from_ref: revspec,
            from_sha: &target_sha,
            from_started_at: from_record.started_at.to_rfc3339(),
            to_started_at: to_record.started_at.to_rfc3339(),
            to_head_sha: to_record.head_sha.as_deref(),
            workspace,
            buckets: &diff,
        });
        return Ok(());
    }

    let mut stdout = std::io::stdout();
    let colorize = stdout.is_terminal();
    render_diff(
        revspec,
        &target_sha,
        workspace,
        &from_record,
        &to_record,
        &diff,
        show_all,
        colorize,
        &mut stdout,
    )?;
    Ok(())
}

/// Walk `.heal/checks/` newest-first and return the first record whose
/// `head_sha` matches the target. Returns `None` when no match exists.
fn find_cached_record(paths: &HealPaths, target_sha: &str) -> Result<Option<CheckRecord>> {
    let records = iter_records(&paths.checks_dir())?;
    Ok(records.into_iter().find_map(|(_, record)| {
        if record.head_sha.as_deref() == Some(target_sha) {
            Some(record)
        } else {
            None
        }
    }))
}

fn build_live_record(project: &Path, paths: &HealPaths) -> Result<CheckRecord> {
    let cfg = load_from_project(project).with_context(|| {
        format!(
            "loading {} (run `heal init` first?)",
            paths.config().display(),
        )
    })?;
    Ok(crate::commands::status::build_live_record(
        project, paths, &cfg,
    ))
}

#[derive(Debug, Serialize)]
struct DiffReport<'a> {
    /// User-supplied revspec ("HEAD", "main", "v0.2.1", "abc1234").
    from_ref: &'a str,
    /// Full 40-char SHA the revspec resolved to.
    from_sha: &'a str,
    from_started_at: String,
    to_started_at: String,
    to_head_sha: Option<&'a str>,
    /// Echo of the user-supplied `--workspace <path>` filter, when any.
    /// Skipped from JSON when omitted so the unfiltered shape stays
    /// terse.
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace: Option<&'a str>,
    #[serde(flatten)]
    buckets: &'a Diff,
}

#[derive(Debug, Clone, Serialize, Default)]
pub(crate) struct Diff {
    pub resolved: Vec<DiffEntry>,
    pub regressed: Vec<DiffEntry>,
    pub improved: Vec<DiffEntry>,
    pub new_findings: Vec<DiffEntry>,
    pub unchanged: Vec<DiffEntry>,
    /// `resolved.len() / from.findings.len()` as a `[0.0, 1.0]` ratio.
    /// `total` is the prior-run finding count.
    pub progress_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DiffEntry {
    pub finding_id: String,
    pub metric: String,
    pub file: String,
    pub from_severity: Option<Severity>,
    pub to_severity: Option<Severity>,
    pub hotspot: bool,
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

pub(crate) fn compute_diff(from: &CheckRecord, to: &CheckRecord, workspace: Option<&str>) -> Diff {
    let in_scope = |f: &Finding| -> bool {
        match workspace {
            None => true,
            Some(ws) => f.workspace.as_deref() == Some(ws),
        }
    };
    let from_by_id: HashMap<&str, &Finding> = from
        .findings
        .iter()
        .filter(|f| in_scope(f))
        .map(|f| (f.id.as_str(), f))
        .collect();
    let to_by_id: HashMap<&str, &Finding> = to
        .findings
        .iter()
        .filter(|f| in_scope(f))
        .map(|f| (f.id.as_str(), f))
        .collect();

    let mut diff = Diff::default();

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

    let total = from_by_id.len();
    diff.progress_pct = if total == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        let pct = diff.resolved.len() as f64 / total as f64;
        pct
    };
    diff
}

#[allow(clippy::too_many_arguments)]
fn render_diff(
    revspec: &str,
    from_sha: &str,
    workspace: Option<&str>,
    from: &CheckRecord,
    to: &CheckRecord,
    diff: &Diff,
    show_all: bool,
    colorize: bool,
    out: &mut impl Write,
) -> Result<()> {
    let title = ansi_wrap(ANSI_CYAN, "── HEAL diff", colorize);
    let bar: String = "─".repeat(48);
    let short = &from_sha[..from_sha.len().min(8)];
    writeln!(out, "{title} {bar}")?;
    if let Some(ws) = workspace {
        writeln!(out, "  workspace: {ws}")?;
    }
    writeln!(
        out,
        "  from: {revspec} ({short})  recorded {}  ({} findings)",
        from.started_at.format("%Y-%m-%d %H:%M"),
        scoped_count(&from.findings, workspace),
    )?;
    writeln!(
        out,
        "  to:   live scan  HEAD={}  ({} findings)",
        to.head_sha.as_deref().unwrap_or("∅"),
        scoped_count(&to.findings, workspace),
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
    let total = scoped_count(&from.findings, workspace);
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

fn scoped_count(findings: &[Finding], workspace: Option<&str>) -> usize {
    match workspace {
        None => findings.len(),
        Some(ws) => findings
            .iter()
            .filter(|f| f.workspace.as_deref() == Some(ws))
            .count(),
    }
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
    use crate::core::check_cache::CheckRecord;
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
            let mut f = finding("hot", Severity::Critical);
            f.severity = Severity::Critical;
            assert_eq!(f.id, regressed_a.id);
            f
        };
        let new_one = finding("new", Severity::High);

        let from = record(vec![dropped.clone(), regressed_a.clone(), stay.clone()]);
        let to = record(vec![regressed_b.clone(), stay.clone(), new_one.clone()]);

        let diff = compute_diff(&from, &to, None);

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
        let diff = compute_diff(&record(vec![prev]), &record(vec![curr]), None);
        assert_eq!(diff.improved.len(), 1);
        assert!(diff.regressed.is_empty());
        assert!((diff.progress_pct - 0.0).abs() < 1e-9);
    }

    #[test]
    fn diff_workspace_filter_scopes_buckets_and_total() {
        // Two findings per side, one in each workspace.
        let mut a_prev = finding("alpha", Severity::High);
        a_prev.workspace = Some("packages/web".into());
        let mut a_curr = finding("alpha", Severity::High);
        a_curr.workspace = Some("packages/web".into());
        let mut b_prev = finding("beta", Severity::High);
        b_prev.workspace = Some("packages/api".into());
        // beta resolved on the new side — but only counts when api scope active.
        let from = record(vec![a_prev, b_prev]);
        let to = record(vec![a_curr]);

        let web = compute_diff(&from, &to, Some("packages/web"));
        assert!(web.resolved.is_empty());
        assert_eq!(web.unchanged.len(), 1);
        // total = 1 (web only); 0 resolved → 0%.
        assert!((web.progress_pct - 0.0).abs() < 1e-9);

        let api = compute_diff(&from, &to, Some("packages/api"));
        assert_eq!(api.resolved.len(), 1);
        assert!(api.unchanged.is_empty());
        // total = 1 (api only); 1 resolved → 100%.
        assert!((api.progress_pct - 1.0).abs() < 1e-9);
    }

    #[test]
    fn diff_progress_zero_when_prior_empty() {
        let diff = compute_diff(
            &record(Vec::new()),
            &record(vec![finding("only", Severity::High)]),
            None,
        );
        assert!((diff.progress_pct - 0.0).abs() < 1e-9);
        assert_eq!(diff.new_findings.len(), 1);
    }
}
