//! `heal diff [<git-ref>]` — bucket-style diff between the current
//! findings and the findings recomputed at an arbitrary git ref.
//! Default ref: the calibration baseline (`meta.calibrated_at_sha`,
//! recorded by `heal init` / `heal calibrate --force`), falling back to
//! `HEAD` when no baseline SHA is recorded — so "Progress: N% complete"
//! reads naturally as "drained since calibration".
//!
//! Two paths:
//!
//! 1. **Cache hit.** `latest.json.head_sha` matches the resolved ref
//!    → read the cached `FindingsRecord` directly. Fast.
//! 2. **Worktree fallback.** `git worktree add --detach <tempdir> <sha>`
//!    materialises the source at the ref, runs the observer pipeline
//!    against it (using the *current* `config.toml`/`calibration.toml`
//!    so the comparison is apples-to-apples), and removes the worktree
//!    on the way out. Gated by `[diff].max_loc_threshold` (default
//!    `200_000` LOC) — over the threshold the command exits with code 2
//!    and points at the manual two-branch flow instead of running an
//!    expensive scan.
//!
//! Output buckets — Resolved / Regressed / Improved / New / Unchanged —
//! plus a progress percentage. JSON shape is stable for skills and CI.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use tempfile::TempDir;

use crate::core::calibration::Calibration;
use crate::core::config::{load_from_project, Config};
use crate::core::finding::Finding;
use crate::core::findings_cache::{read_latest, FindingsRecord};
use crate::core::severity::Severity;
use crate::core::term::{
    ansi_wrap, write_through_pager, ANSI_CYAN, ANSI_GREEN, ANSI_RED, ANSI_YELLOW,
};
use crate::core::HealPaths;
use crate::observer::git;
use crate::observer::loc::LocObserver;
use crate::observers::build_record;

/// Exit status when `[diff].max_loc_threshold` is exceeded. Wraps the
/// human-readable guidance in a `process::exit` so scripts can branch on
/// the code without parsing stderr.
pub const DIFF_LOC_THRESHOLD_EXIT_CODE: i32 = 2;

pub fn run(
    project: &Path,
    revspec: Option<&str>,
    workspace: Option<&str>,
    show_all: bool,
    as_json: bool,
    no_pager: bool,
) -> Result<()> {
    let paths = HealPaths::new(project);
    let cfg = load_from_project(project).with_context(|| {
        format!(
            "loading {} (run `heal init` first?)",
            paths.config().display(),
        )
    })?;
    // No positional ref → diff against the calibration baseline so the
    // progress percentage reads as "drained since `heal init` /
    // `heal calibrate --force`". Falls back to `HEAD` if no baseline
    // SHA was recorded (e.g. calibration produced outside a git
    // worktree).
    let resolved_ref = match revspec {
        Some(r) => r.to_owned(),
        None => Calibration::load(&paths.calibration())
            .ok()
            .and_then(|c| c.meta.calibrated_at_sha)
            .unwrap_or_else(|| "HEAD".to_owned()),
    };
    let target_sha = git::resolve_ref(project, &resolved_ref).ok_or_else(|| {
        anyhow!(
            "could not resolve git ref `{resolved_ref}` in {} — is this a git repo?",
            project.display(),
        )
    })?;

    let from_record = load_or_recompute_from(project, &paths, &cfg, &resolved_ref, &target_sha)?;
    let to_head_sha = git::head_sha(project);
    let to_clean = git::worktree_clean(project).unwrap_or(false);
    let to_record = build_record(project, &paths, &cfg, to_head_sha, to_clean);

    let diff = compute_diff(&from_record, &to_record, workspace);
    if as_json {
        super::emit_json(&DiffReport {
            from_ref: &resolved_ref,
            from_sha: &target_sha,
            from_started_at: from_record.started_at.to_rfc3339(),
            to_started_at: to_record.started_at.to_rfc3339(),
            to_head_sha: to_record.head_sha.as_deref(),
            workspace,
            buckets: &diff,
        });
        return Ok(());
    }

    write_through_pager(no_pager, |out, colorize| {
        render_diff(
            &resolved_ref,
            &target_sha,
            workspace,
            &from_record,
            &to_record,
            &diff,
            show_all,
            colorize,
            out,
        )
    })
}

/// Build the "from" `FindingsRecord`. Prefers the cached `latest.json` when
/// its `head_sha` matches the target; otherwise materialises the source
/// at `<sha>` in a tempdir-backed `git worktree` and runs the observer
/// pipeline against it (after the LOC threshold check).
fn load_or_recompute_from(
    project: &Path,
    paths: &HealPaths,
    cfg: &Config,
    revspec: &str,
    target_sha: &str,
) -> Result<FindingsRecord> {
    if let Some(record) =
        read_latest(&paths.findings_latest())?.filter(|r| r.head_sha.as_deref() == Some(target_sha))
    {
        return Ok(record);
    }
    enforce_loc_threshold(project, cfg, revspec);
    recompute_at_ref(project, paths, cfg, target_sha)
}

/// Run a fast LOC count on the *current* worktree as a proxy for the
/// expected scan cost at `<sha>` (repos rarely change LOC by orders of
/// magnitude between commits). Returns when under the threshold or
/// exits the process with [`DIFF_LOC_THRESHOLD_EXIT_CODE`] otherwise.
fn enforce_loc_threshold(project: &Path, cfg: &Config, revspec: &str) {
    let report = LocObserver::from_config(cfg).scan(project);
    let total_loc = report.totals.code;
    let threshold = cfg.diff.max_loc_threshold;
    if u64::try_from(total_loc).unwrap_or(u64::MAX) <= threshold {
        return;
    }
    eprintln!("heal diff: project LOC {total_loc} exceeds [diff].max_loc_threshold ({threshold}).");
    eprintln!("Run two scans by hand instead:");
    eprintln!("  git worktree add --detach <tmp> {revspec}");
    eprintln!("  (cd <tmp> && heal status --refresh --json) > from.json");
    eprintln!("  heal status --refresh --json                > to.json");
    eprintln!("  # diff the two JSON payloads with your tool of choice");
    eprintln!("  git worktree remove <tmp>");
    eprintln!("Or raise the threshold in `.heal/config.toml` under `[diff]`.");
    std::process::exit(DIFF_LOC_THRESHOLD_EXIT_CODE);
}

/// Materialise `<sha>` in a fresh `git worktree`, run the observer
/// pipeline against it, and tear the worktree down. Uses the host
/// project's `.heal/calibration.toml` and `.heal/config.toml` so the
/// "from" record is comparable apples-to-apples with the live "to"
/// record under current rules.
fn recompute_at_ref(
    project: &Path,
    paths: &HealPaths,
    cfg: &Config,
    target_sha: &str,
) -> Result<FindingsRecord> {
    let tmp = TempDir::new().context("creating tempdir for `git worktree add`")?;
    let workdir = tmp.path().join("heal-diff");
    let _guard = WorktreeGuard::add(project, &workdir, target_sha)?;
    // A fresh `git worktree add --detach` is clean by construction.
    Ok(build_record(
        &workdir,
        paths,
        cfg,
        Some(target_sha.to_owned()),
        true,
    ))
}

/// RAII handle for a transient `git worktree`. `add` runs
/// `git worktree add --detach <path> <sha>` against the host project;
/// `Drop` runs `git worktree remove --force <path>` so a panic or `?`
/// short-circuit doesn't leave `.git/worktrees/` polluted.
struct WorktreeGuard {
    project: PathBuf,
    workdir: PathBuf,
}

impl WorktreeGuard {
    fn add(project: &Path, workdir: &Path, target_sha: &str) -> Result<Self> {
        if let Some(parent) = workdir.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let status = Command::new("git")
            .arg("-C")
            .arg(project)
            .args(["worktree", "add", "--detach", "--force"])
            .arg(workdir)
            .arg(target_sha)
            .status()
            .context("invoking `git worktree add`")?;
        if !status.success() {
            return Err(anyhow!(
                "`git worktree add` failed for {} at {target_sha}",
                workdir.display(),
            ));
        }
        Ok(Self {
            project: project.to_path_buf(),
            workdir: workdir.to_path_buf(),
        })
    }
}

impl Drop for WorktreeGuard {
    fn drop(&mut self) {
        let _ = Command::new("git")
            .arg("-C")
            .arg(&self.project)
            .args(["worktree", "remove", "--force"])
            .arg(&self.workdir)
            .status();
    }
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

pub(crate) fn compute_diff(
    from: &FindingsRecord,
    to: &FindingsRecord,
    workspace: Option<&str>,
) -> Diff {
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
    from: &FindingsRecord,
    to: &FindingsRecord,
    diff: &Diff,
    show_all: bool,
    colorize: bool,
    out: &mut (impl Write + ?Sized),
) -> Result<()> {
    let short = &from_sha[..from_sha.len().min(8)];
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
    out: &mut (impl Write + ?Sized),
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
    use crate::core::findings_cache::FindingsRecord;
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

    fn record(findings: Vec<Finding>) -> FindingsRecord {
        FindingsRecord::new(Some("abc".into()), true, "h".into(), findings)
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

    #[test]
    fn recompute_at_ref_materialises_worktree_and_runs_observers() {
        use crate::core::config::Config;
        use crate::test_support::{commit, init_repo};
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(
            dir.path(),
            "lib.rs",
            "fn ok() {}\n",
            "tester@example.com",
            "init",
        );
        let head_sha = git::head_sha(dir.path()).expect("head sha after first commit");
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        Config::default().save(&paths.config()).unwrap();

        let cfg = load_from_project(dir.path()).unwrap();
        let record = recompute_at_ref(dir.path(), &paths, &cfg, &head_sha).unwrap();
        assert_eq!(record.head_sha.as_deref(), Some(head_sha.as_str()));
        // Sanity: the worktree was torn down and didn't leak.
        let leftover = std::fs::read_dir(dir.path().join(".git/worktrees"))
            .ok()
            .map_or(0, std::iter::Iterator::count);
        assert_eq!(leftover, 0, "git worktree must be cleaned up after diff");
    }
}
