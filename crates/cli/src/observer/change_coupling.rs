//! Co-change analysis: which files tend to be modified together?
//!
//! For every reachable commit inside the `since_days` window, we extract
//! the set of paths it touches (first-parent diff to avoid merge-driven
//! double-counting) and increment a counter for every unordered pair in
//! that set. The pair counters become the "coupling" signal — files that
//! consistently change together expose hidden behavioural coupling that
//! static analysis can't see (KNOWLEDGE.md § 3.5).
//!
//! Bulk commits (lockfile bumps, mass renames, generated-code refreshes)
//! would otherwise dominate the pair-count quadratic blow-up. We hard-cap
//! the per-commit fan-out at `BULK_COMMIT_FILE_LIMIT`; configurable knob
//! is deferred to v0.2.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use git2::{Repository, Sort};
use serde::{Deserialize, Serialize};

use crate::core::config::Config;
use crate::core::finding::{Finding, IntoFindings, Location};

use crate::observer::walk::{is_path_excluded, since_cutoff};
use crate::observer::{ObservationMeta, Observer};

/// Skip commits whose changed-file count exceeds this limit. The pair count
/// grows O(N²) per commit so bulk merges (think 200-file lockfile bumps)
/// would otherwise drown the signal. v0.2 may expose this as TOML config.
const BULK_COMMIT_FILE_LIMIT: usize = 50;

#[derive(Debug, Clone, Default)]
pub struct ChangeCouplingObserver {
    pub enabled: bool,
    pub excluded: Vec<String>,
    pub since_days: u32,
    /// Pairs with fewer than `min_coupling` co-occurrences are dropped from
    /// the report. Sourced from `metrics.change_coupling.min_coupling`.
    pub min_coupling: u32,
}

impl ChangeCouplingObserver {
    #[must_use]
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            enabled: cfg.metrics.change_coupling.enabled,
            excluded: cfg.observer_excluded_paths(),
            since_days: cfg.git.since_days,
            min_coupling: cfg.metrics.change_coupling.min_coupling,
        }
    }

    #[must_use]
    pub fn scan(&self, root: &Path) -> ChangeCouplingReport {
        let mut report = ChangeCouplingReport {
            since_days: self.since_days,
            min_coupling: self.min_coupling,
            ..ChangeCouplingReport::default()
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

        let mut pair_counts: HashMap<(PathBuf, PathBuf), u32> = HashMap::new();
        let mut commits_considered: u32 = 0;

        for oid_res in revwalk {
            let Ok(oid) = oid_res else {
                continue;
            };
            let Ok(commit) = repo.find_commit(oid) else {
                continue;
            };
            if commit.time().seconds() < cutoff_secs {
                break;
            }
            if self.absorb_commit(&repo, &commit, &mut pair_counts) {
                commits_considered = commits_considered.saturating_add(1);
            }
        }

        let pairs = collect_pairs(pair_counts, self.min_coupling);
        let file_sums = compute_file_sums(&pairs);

        let totals = CouplingTotals {
            pairs: pairs.len(),
            files: file_sums.len(),
            commits_considered,
        };
        report.pairs = pairs;
        report.file_sums = file_sums;
        report.totals = totals;
        report
    }

    /// Returns `true` if the commit's filtered changeset contributed pair
    /// counts (i.e. landed within the bulk-commit limit and had ≥2 files).
    fn absorb_commit(
        &self,
        repo: &Repository,
        commit: &git2::Commit<'_>,
        pair_counts: &mut HashMap<(PathBuf, PathBuf), u32>,
    ) -> bool {
        let Ok(commit_tree) = commit.tree() else {
            return false;
        };
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
        let Ok(diff) = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)
        else {
            return false;
        };

        let mut paths: BTreeSet<PathBuf> = BTreeSet::new();
        for delta in diff.deltas() {
            let Some(path) = delta.new_file().path() else {
                continue;
            };
            if path.as_os_str().is_empty() || is_path_excluded(path, &self.excluded) {
                continue;
            }
            paths.insert(path.to_path_buf());
        }
        if paths.len() < 2 || paths.len() > BULK_COMMIT_FILE_LIMIT {
            return false;
        }

        // BTreeSet iterates in sorted order, so the (a, b) pairs we emit are
        // already canonical (a < b).
        let ordered: Vec<&PathBuf> = paths.iter().collect();
        for (i, a) in ordered.iter().enumerate() {
            for b in &ordered[i + 1..] {
                let counter = pair_counts.entry(((*a).clone(), (*b).clone())).or_insert(0);
                *counter = counter.saturating_add(1);
            }
        }
        true
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeCouplingReport {
    /// All co-occurring pairs whose count meets `min_coupling`, sorted by
    /// count descending then path lexicographically.
    pub pairs: Vec<FilePair>,
    /// Per-file sum-of-coupling: the sum of pair counts every pair this
    /// file participates in. Mirrors Tornhill's "sum of coupling" metric.
    pub file_sums: Vec<FileSum>,
    pub totals: CouplingTotals,
    pub since_days: u32,
    pub min_coupling: u32,
}

impl ChangeCouplingReport {
    /// Top-N pairs by co-occurrence count (descending). The underlying
    /// `pairs` vector is already sorted by `collect_pairs`.
    #[must_use]
    pub fn worst_n_pairs(&self, n: usize) -> Vec<FilePair> {
        let mut top = self.pairs.clone();
        top.truncate(n);
        top
    }

    /// Top-N files by sum-of-coupling (descending). The underlying
    /// `file_sums` vector is already sorted by `compute_file_sums`.
    #[must_use]
    pub fn worst_n_files(&self, n: usize) -> Vec<FileSum> {
        let mut top = self.file_sums.clone();
        top.truncate(n);
        top
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilePair {
    /// Always the lexicographically smaller path of the pair.
    pub a: PathBuf,
    pub b: PathBuf,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileSum {
    pub path: PathBuf,
    pub sum: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CouplingTotals {
    pub pairs: usize,
    pub files: usize,
    pub commits_considered: u32,
}

impl Observer for ChangeCouplingObserver {
    type Output = ChangeCouplingReport;

    fn meta(&self) -> ObservationMeta {
        ObservationMeta {
            name: "change_coupling",
            version: 1,
        }
    }

    fn observe(&self, project_root: &Path) -> anyhow::Result<Self::Output> {
        Ok(self.scan(project_root))
    }
}

impl IntoFindings for ChangeCouplingReport {
    /// `a` (lex-smaller, already canonical) is the primary
    /// `location.file`; the partner `b` is the primary's `symbol` (so
    /// a → b and a → c distinguish in `id`) and also a secondary entry
    /// in `locations`. Calibration's symmetric / directional split
    /// (TODO §双方向 Change Coupling) lands later.
    fn into_findings(&self) -> Vec<Finding> {
        self.pairs
            .iter()
            .map(|pair| {
                let b_str = pair.b.to_string_lossy().into_owned();
                let primary = Location {
                    file: pair.a.clone(),
                    line: None,
                    symbol: Some(b_str.clone()),
                };
                let summary = format!(
                    "co-changed {} times: {} ↔ {}",
                    pair.count,
                    pair.a.display(),
                    b_str,
                );
                Finding::new(
                    "change_coupling",
                    primary,
                    summary,
                    &format!("count:{}", pair.count),
                )
                .with_locations(vec![Location::file(pair.b.clone())])
            })
            .collect()
    }
}

fn collect_pairs(
    pair_counts: HashMap<(PathBuf, PathBuf), u32>,
    min_coupling: u32,
) -> Vec<FilePair> {
    let mut pairs: Vec<FilePair> = pair_counts
        .into_iter()
        .filter(|(_, count)| *count >= min_coupling)
        .map(|((a, b), count)| FilePair { a, b, count })
        .collect();
    pairs.sort_by(|x, y| {
        y.count
            .cmp(&x.count)
            .then_with(|| x.a.cmp(&y.a))
            .then_with(|| x.b.cmp(&y.b))
    });
    pairs
}

fn compute_file_sums(pairs: &[FilePair]) -> Vec<FileSum> {
    let mut sums: BTreeMap<PathBuf, u32> = BTreeMap::new();
    for pair in pairs {
        let a = sums.entry(pair.a.clone()).or_insert(0);
        *a = a.saturating_add(pair.count);
        let b = sums.entry(pair.b.clone()).or_insert(0);
        *b = b.saturating_add(pair.count);
    }
    let mut file_sums: Vec<FileSum> = sums
        .into_iter()
        .map(|(path, sum)| FileSum { path, sum })
        .collect();
    file_sums.sort_by(|x, y| y.sum.cmp(&x.sum).then_with(|| x.path.cmp(&y.path)));
    file_sums
}
