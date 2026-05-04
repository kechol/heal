//! Per-pair documentation freshness via git churn distance.
//!
//! For every doc ⇔ src(s) entry in `.heal/doc_pairs.json` the observer
//! answers a single question: **"how many commits has the source side
//! moved since the doc last changed?"** mtime is forbidden by
//! `scope.md` R2, so the signal is computed from `git log` rather than
//! filesystem timestamps — the result is deterministic across teammates
//! who fetched the same commit graph.
//!
//! The metric is intentionally absolute, not percentile-based. Drift
//! risk doesn't reshape per project: ten src commits past a stale doc
//! is "stale" everywhere. Calibration percentiles add noise here, so
//! the floors live in `[features.docs.doc_freshness]` (config.toml,
//! invariants.md R9) and a per-pair distribution is omitted.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use git2::{Repository, Sort};
use serde::{Deserialize, Serialize};

use crate::core::config::Config;
use crate::core::doc_pairs::{DocPair, DocPairsFile};
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::core::severity::Severity;
use crate::feature::{decorate, Family, Feature, FeatureKind, FeatureMeta, HotspotIndex};

/// Stateless observer; constructing one is cheap. The pairs come from
/// the `SSoT` loaded by `observers::run_all` — the observer never reads
/// the JSON file itself, so the pair list is always pre-validated.
#[derive(Debug, Clone, Default)]
pub struct DocFreshnessObserver {
    pub enabled: bool,
    pub pairs: Vec<DocPair>,
    pub high_commits: u32,
    pub critical_commits: u32,
}

impl DocFreshnessObserver {
    #[must_use]
    pub fn from_config_and_pairs(cfg: &Config, pairs: Vec<DocPair>) -> Self {
        Self {
            enabled: cfg.features.docs.enabled,
            pairs,
            high_commits: cfg.features.docs.doc_freshness.high_commits,
            critical_commits: cfg.features.docs.doc_freshness.critical_commits,
        }
    }

    /// Walk HEAD-reachable history and accumulate per-pair commit
    /// counts. Returns an empty report when the feature is disabled,
    /// pairs are empty, or `root` isn't inside a git repo.
    #[must_use]
    pub fn scan(&self, root: &Path) -> DocFreshnessReport {
        let mut report = DocFreshnessReport::default();
        if !self.enabled || self.pairs.is_empty() {
            return report;
        }
        let Ok(repo) = Repository::discover(root) else {
            return report;
        };

        // Collect "interesting" paths once — every path mentioned by any
        // pair, on either side. Used to short-circuit the diff walk so
        // we don't pay full diff cost on commits that don't touch any
        // tracked doc / src.
        let mut watched: HashSet<PathBuf> = HashSet::new();
        for pair in &self.pairs {
            watched.insert(PathBuf::from(&pair.doc));
            for src in &pair.srcs {
                watched.insert(PathBuf::from(src));
            }
        }

        let Ok(mut revwalk) = repo.revwalk() else {
            return report;
        };
        if revwalk.set_sorting(Sort::TIME).is_err() || revwalk.push_head().is_err() {
            return report;
        }

        // For each watched path, collect the timestamps of every commit
        // touching it. Newest-first order matches the revwalk; we sort
        // descending below so binary-search style "newer than X" checks
        // are simple.
        let mut commits_by_path: BTreeMap<PathBuf, Vec<i64>> = BTreeMap::new();
        for oid_res in revwalk {
            let Ok(oid) = oid_res else {
                continue;
            };
            let Ok(commit) = repo.find_commit(oid) else {
                continue;
            };
            let when = commit.time().seconds();
            absorb_commit(&repo, &commit, &watched, when, &mut commits_by_path);
        }

        // Per pair: doc's last commit time = max over its single doc
        // path. Src commits since then = count of commits touching any
        // src whose timestamp is strictly greater than that mark.
        let mut entries: Vec<DocFreshnessEntry> = Vec::with_capacity(self.pairs.len());
        for pair in &self.pairs {
            let doc_last = commits_by_path
                .get(&PathBuf::from(&pair.doc))
                .and_then(|v| v.iter().copied().max());
            let src_commits_since_doc = match doc_last {
                Some(mark) => count_src_commits_after(&commits_by_path, &pair.srcs, mark),
                None => {
                    // Doc has no history yet — skip, since the
                    // "freshness" question is undefined. The doc is
                    // surfaced by `doc_coverage` instead when the file
                    // doesn't exist, or it's just an unstaged file.
                    0
                }
            };
            entries.push(DocFreshnessEntry {
                doc_path: PathBuf::from(&pair.doc),
                src_paths: pair.srcs.iter().map(PathBuf::from).collect(),
                src_commits_since_doc,
                doc_last_commit_time: doc_last,
            });
        }
        // Stable order: most-stale first, then by doc path.
        entries.sort_by(|a, b| {
            b.src_commits_since_doc
                .cmp(&a.src_commits_since_doc)
                .then_with(|| a.doc_path.cmp(&b.doc_path))
        });
        let stale_pairs = entries
            .iter()
            .filter(|e| e.src_commits_since_doc > 0)
            .count();
        report.totals = DocFreshnessTotals {
            pairs: entries.len(),
            stale_pairs,
        };
        report.entries = entries;
        report
    }

    /// Apply config floors to a single per-pair commit count. Kept
    /// public so the per-metric tests can share the rule.
    #[must_use]
    pub fn classify(&self, src_commits_since_doc: u32) -> Severity {
        classify_freshness(
            src_commits_since_doc,
            self.high_commits,
            self.critical_commits,
        )
    }
}

/// Map `src_commits_since_doc` to a Severity using the supplied
/// floors. Free-function so the Feature lowering path doesn't need to
/// reconstruct an observer just to call it.
#[must_use]
pub fn classify_freshness(
    src_commits_since_doc: u32,
    high_commits: u32,
    critical_commits: u32,
) -> Severity {
    if src_commits_since_doc >= critical_commits {
        Severity::Critical
    } else if src_commits_since_doc >= high_commits {
        Severity::High
    } else if src_commits_since_doc > 0 {
        Severity::Medium
    } else {
        Severity::Ok
    }
}

/// Diff `commit` against its first parent and append the commit time
/// to every watched path it touched. Mirrors `ChurnObserver::absorb_commit`
/// but only records timestamps — line counts aren't needed for drift.
fn absorb_commit(
    repo: &Repository,
    commit: &git2::Commit<'_>,
    watched: &HashSet<PathBuf>,
    when: i64,
    commits_by_path: &mut BTreeMap<PathBuf, Vec<i64>>,
) {
    let Ok(commit_tree) = commit.tree() else {
        return;
    };
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let Ok(diff) = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None) else {
        return;
    };

    let mut paths_in_commit: BTreeSet<PathBuf> = BTreeSet::new();
    for delta in diff.deltas() {
        let Some(path) = delta.new_file().path() else {
            continue;
        };
        if path.as_os_str().is_empty() {
            continue;
        }
        let pb = path.to_path_buf();
        if watched.contains(&pb) {
            paths_in_commit.insert(pb);
        }
    }
    for path in paths_in_commit {
        commits_by_path.entry(path).or_default().push(when);
    }
}

/// Number of distinct commits whose timestamp is **strictly greater**
/// than `mark` and that touched any of the supplied src paths. Distinct
/// because two srcs co-modified in one commit should still count as
/// one "src side commit since doc".
fn count_src_commits_after(
    commits_by_path: &BTreeMap<PathBuf, Vec<i64>>,
    srcs: &[String],
    mark: i64,
) -> u32 {
    // Dedupe by timestamp — a commit touching multiple srcs of the
    // same pair is one bump, not N. Two distinct commits sharing a
    // second is a degenerate case we accept the under-count on.
    let mut seen: BTreeSet<i64> = BTreeSet::new();
    for src in srcs {
        let key = PathBuf::from(src);
        if let Some(times) = commits_by_path.get(&key) {
            for &t in times {
                if t > mark {
                    seen.insert(t);
                }
            }
        }
    }
    u32::try_from(seen.len()).unwrap_or(u32::MAX)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocFreshnessReport {
    pub entries: Vec<DocFreshnessEntry>,
    pub totals: DocFreshnessTotals,
}

impl DocFreshnessReport {
    /// Top-N pairs ordered by `src_commits_since_doc` desc.
    #[must_use]
    pub fn worst_n(&self, n: usize) -> Vec<DocFreshnessEntry> {
        let mut top = self.entries.clone();
        top.truncate(n);
        top
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocFreshnessEntry {
    pub doc_path: PathBuf,
    pub src_paths: Vec<PathBuf>,
    pub src_commits_since_doc: u32,
    /// Unix-second timestamp of the doc's last commit, when known.
    /// Skipped from JSON when absent so the shape stays compact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_last_commit_time: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocFreshnessTotals {
    pub pairs: usize,
    pub stale_pairs: usize,
}

impl IntoFindings for DocFreshnessReport {
    /// One finding per pair with `src_commits_since_doc > 0`. Severity
    /// stays `Ok` here — the Feature pass classifies against config
    /// floors. Multi-src pairs surface every src in `locations`.
    fn into_findings(&self) -> Vec<Finding> {
        self.entries
            .iter()
            .filter(|e| e.src_commits_since_doc > 0)
            .map(|entry| {
                let primary = Location::file(entry.doc_path.clone());
                let locations: Vec<Location> = entry
                    .src_paths
                    .iter()
                    .map(|p| Location::file(p.clone()))
                    .collect();
                let summary = format!(
                    "doc_freshness: src has moved {} commit(s) since doc last changed",
                    entry.src_commits_since_doc,
                );
                let seed = format!(
                    "doc_freshness:{}:{}",
                    entry.doc_path.to_string_lossy(),
                    entry
                        .src_paths
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join(",")
                );
                Finding::new("doc_freshness", primary, summary, &seed).with_locations(locations)
            })
            .collect()
    }
}

/// Pull the pair list out of an [`ObserverReports`]'s `SSoT`, filtered to
/// only those entries whose paths exist on disk. Lets callers (the
/// observer + tests) share one resolution rule.
///
/// [`ObserverReports`]: crate::observers::ObserverReports
#[must_use]
pub fn live_pairs(file: Option<&DocPairsFile>, project: &Path) -> Vec<DocPair> {
    file.map(|f| f.live_pairs(project).into_iter().cloned().collect())
        .unwrap_or_default()
}

pub struct DocFreshnessFeature;

impl Feature for DocFreshnessFeature {
    fn meta(&self) -> FeatureMeta {
        FeatureMeta {
            name: "doc_freshness",
            version: 1,
            kind: FeatureKind::DocsScanner,
        }
    }
    fn enabled(&self, cfg: &Config) -> bool {
        cfg.features.docs.enabled
    }
    fn family(&self) -> Family {
        Family::Docs
    }
    fn lower(
        &self,
        reports: &crate::observers::ObserverReports,
        cfg: &Config,
        _cal: &crate::core::calibration::Calibration,
        hotspot: &HotspotIndex,
    ) -> Vec<Finding> {
        let Some(report) = reports.doc_freshness.as_ref() else {
            return Vec::new();
        };
        let high = cfg.features.docs.doc_freshness.high_commits;
        let critical = cfg.features.docs.doc_freshness.critical_commits;
        report
            .into_findings()
            .into_iter()
            .zip(
                report
                    .entries
                    .iter()
                    .filter(|e| e.src_commits_since_doc > 0),
            )
            .map(|(finding, entry)| {
                let severity = classify_freshness(entry.src_commits_since_doc, high, critical);
                decorate(finding, severity, hotspot)
            })
            .collect()
    }
}
