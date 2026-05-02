//! Per-file change frequency over a sliding time window.
//!
//! Walks the HEAD-reachable history with `git2::Revwalk`, restricted to
//! commits whose author/commit time is within `git.since_days`. For every
//! reachable commit we diff against its **first parent** only — merge
//! commits are otherwise counted twice (once per parent line) and inflate
//! the churn signal.
//!
//! v0.1 deliberately does *not* enable rename/copy detection; the metric
//! tracks paths verbatim so it stays stable and cheap. Renames will be
//! folded in later (TODO.md → v0.2).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use git2::{DiffFormat, Oid, Repository, Sort};
use serde::{Deserialize, Serialize};

use crate::core::config::Config;

use crate::observer::walk::{path_under, resolve_workspace_target, since_cutoff, ExcludeMatcher};
use crate::observer::{impl_workspace_builder, ObservationMeta, Observer};

impl_workspace_builder!(ChurnObserver);

/// Observer toggle + window inputs. Stateless; constructing one is cheap.
#[derive(Debug, Clone, Default)]
pub struct ChurnObserver {
    pub enabled: bool,
    /// Substrings checked against every diffed path; matches are skipped.
    /// Mirrors `LocObserver::excluded` for consistency.
    pub excluded: Vec<String>,
    /// Inclusive lookback window in days, sourced from `git.since_days`.
    pub since_days: u32,
    /// Optional workspace sub-path. When set, files outside drop from
    /// per-file rows AND `totals.commits` recounts to "commits that
    /// touched ≥1 in-workspace file" so the number reflects activity
    /// in this workspace specifically.
    pub workspace: Option<PathBuf>,
}

impl ChurnObserver {
    #[must_use]
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            enabled: cfg.metrics.churn.enabled,
            excluded: cfg.exclude_lines(),
            since_days: cfg.git.since_days,
            workspace: None,
        }
    }

    /// Walk the repository at (or above) `root` and accumulate per-file
    /// commit counts and changed-line totals. Returns a `since_days`-stamped
    /// empty report if `root` isn't inside a git repo or if churn is
    /// disabled, so callers can treat the result uniformly.
    #[must_use]
    pub fn scan(&self, root: &Path) -> ChurnReport {
        let mut report = ChurnReport {
            since_days: self.since_days,
            ..ChurnReport::default()
        };
        if !self.enabled {
            return report;
        }
        let Ok(repo) = Repository::discover(root) else {
            return report;
        };

        let cutoff_secs = since_cutoff(self.since_days);
        let Ok(mut revwalk) = repo.revwalk() else {
            return report;
        };
        if revwalk.set_sorting(Sort::TIME).is_err() || revwalk.push_head().is_err() {
            return report;
        }

        let mut per_file: BTreeMap<PathBuf, FileChurn> = BTreeMap::new();
        let mut commit_oids: HashSet<Oid> = HashSet::new();
        // git2 yields paths relative to the repo root, so the workspace
        // target is resolved as relative too — no per-call `root.join`
        // inside the diff loop.
        let workspace_target = resolve_workspace_target(root, self.workspace.as_deref(), false);
        let matcher = ExcludeMatcher::compile(root, &self.excluded)
            .expect("exclude patterns validated at config load");

        for oid_res in revwalk {
            let Ok(oid) = oid_res else {
                continue;
            };
            let Ok(commit) = repo.find_commit(oid) else {
                continue;
            };
            if commit.time().seconds() < cutoff_secs {
                // Sort::TIME yields newest first; once we drop past the
                // window we're done.
                break;
            }
            let contributed = Self::absorb_commit(
                &repo,
                &commit,
                workspace_target.as_deref(),
                &matcher,
                &mut per_file,
            );
            // Without a workspace filter, count every in-window commit;
            // with one, count only commits that touched ≥1 in-workspace
            // file so the number reflects activity *in* this workspace.
            if self.workspace.is_none() || contributed {
                commit_oids.insert(oid);
            }
        }

        let mut files: Vec<FileChurn> = per_file.into_values().collect();
        files.sort_by(|a, b| b.commits.cmp(&a.commits).then_with(|| a.path.cmp(&b.path)));

        let totals = ChurnTotals {
            files: files.len(),
            commits: u32::try_from(commit_oids.len()).unwrap_or(u32::MAX),
            lines_added: files.iter().map(|f| f.lines_added).sum(),
            lines_deleted: files.iter().map(|f| f.lines_deleted).sum(),
        };
        report.files = files;
        report.totals = totals;
        report
    }

    /// Diff `commit` against its first parent and fold per-file commit
    /// counts + line stats into `per_file`. Errors and zero-delta commits
    /// are silently skipped — churn is best-effort over historical data.
    ///
    /// Returns `true` if at least one path passed every filter (exclude
    /// list and optional workspace) and contributed to `per_file`.
    /// Callers use the bool to decide whether the commit should count
    /// toward `totals.commits` under workspace scoping.
    fn absorb_commit(
        repo: &Repository,
        commit: &git2::Commit<'_>,
        workspace_target: Option<&Path>,
        matcher: &ExcludeMatcher,
        per_file: &mut BTreeMap<PathBuf, FileChurn>,
    ) -> bool {
        let Ok(commit_tree) = commit.tree() else {
            return false;
        };
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
        let Ok(diff) = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)
        else {
            return false;
        };

        let mut paths_in_commit: BTreeSet<PathBuf> = BTreeSet::new();
        for delta in diff.deltas() {
            let Some(path) = delta.new_file().path() else {
                continue;
            };
            if path.as_os_str().is_empty() {
                continue;
            }
            if !path_under(path, workspace_target) {
                continue;
            }
            // git2 yields files only (no dir deltas), so is_dir = false.
            if matcher.is_excluded(path, false) {
                continue;
            }
            paths_in_commit.insert(path.to_path_buf());
        }
        if paths_in_commit.is_empty() {
            return false;
        }

        let mut local_added: HashMap<PathBuf, u32> = HashMap::new();
        let mut local_deleted: HashMap<PathBuf, u32> = HashMap::new();
        let _ = diff.print(DiffFormat::Patch, |delta, _hunk, line| {
            let Some(path) = delta.new_file().path() else {
                return true;
            };
            if path.as_os_str().is_empty() {
                return true;
            }
            let path = path.to_path_buf();
            if !paths_in_commit.contains(&path) {
                return true;
            }
            match line.origin() {
                '+' => {
                    let c = local_added.entry(path).or_insert(0);
                    *c = c.saturating_add(1);
                }
                '-' => {
                    let c = local_deleted.entry(path).or_insert(0);
                    *c = c.saturating_add(1);
                }
                _ => {}
            }
            true
        });

        for path in &paths_in_commit {
            let entry = per_file
                .entry(path.clone())
                .or_insert_with(|| FileChurn::new(path.clone()));
            entry.commits = entry.commits.saturating_add(1);
            entry.lines_added = entry
                .lines_added
                .saturating_add(local_added.get(path).copied().unwrap_or(0));
            entry.lines_deleted = entry
                .lines_deleted
                .saturating_add(local_deleted.get(path).copied().unwrap_or(0));
        }
        true
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChurnReport {
    pub files: Vec<FileChurn>,
    pub totals: ChurnTotals,
    pub since_days: u32,
}

impl ChurnReport {
    /// Top-N files ordered by commit count (descending), with path as a
    /// deterministic tie-breaker. Mirrors `ComplexityReport::worst_n`.
    #[must_use]
    pub fn worst_n(&self, n: usize) -> Vec<FileChurn> {
        let mut top: Vec<FileChurn> = self.files.clone();
        top.sort_by(|a, b| b.commits.cmp(&a.commits).then_with(|| a.path.cmp(&b.path)));
        top.truncate(n);
        top
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileChurn {
    pub path: PathBuf,
    pub commits: u32,
    pub lines_added: u32,
    pub lines_deleted: u32,
}

impl FileChurn {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            commits: 0,
            lines_added: 0,
            lines_deleted: 0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChurnTotals {
    pub files: usize,
    pub commits: u32,
    pub lines_added: u32,
    pub lines_deleted: u32,
}

impl Observer for ChurnObserver {
    type Output = ChurnReport;

    fn meta(&self) -> ObservationMeta {
        ObservationMeta {
            name: "churn",
            version: 1,
        }
    }

    fn observe(&self, project_root: &Path) -> anyhow::Result<Self::Output> {
        Ok(self.scan(project_root))
    }
}
