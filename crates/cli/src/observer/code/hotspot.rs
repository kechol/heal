//! Hotspot composite metric: technical debt priority scored as the
//! product of churn (commit count) and complexity (per-file CCN sum),
//! optionally re-weighted via `metrics.hotspot.weight_*`.
//!
//! The arithmetic is intentionally simple — `(weight_complexity *
//! ccn_sum) * (weight_churn * commits)` — following Tornhill's
//! "true risk = volatility × complexity" framing: a file scores high
//! only when both factors are high, so files that lack a churn or
//! complexity contribution get a score of 0 and drop out of the report.
//!
//! `compose` is a pure function over already-computed reports so the
//! `status` command path can reuse the work the Churn/Complexity
//! observers already did. `Observer::observe` re-runs both observers from
//! the project root for callers that don't have reports on hand.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::config::{Config, DocFreshnessConfig};
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::core::severity::Severity;
use crate::feature::{decorate, Feature, FeatureKind, FeatureMeta, HotspotIndex};

use crate::observer::code::churn::{ChurnObserver, ChurnReport, FileChurn};
use crate::observer::code::complexity::{ComplexityObserver, ComplexityReport, FileComplexity};
use crate::observer::docs::freshness::DocFreshnessReport;
use crate::observer::test::coverage::CoverageReport;
use crate::observer::{ObservationMeta, Observer};
use crate::observers::ObserverReports;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HotspotWeights {
    pub churn: f64,
    pub complexity: f64,
}

impl Default for HotspotWeights {
    fn default() -> Self {
        Self {
            churn: 1.0,
            complexity: 1.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HotspotObserver {
    pub enabled: bool,
    pub weights: HotspotWeights,
    /// Used by `Observer::observe` to scan the project end-to-end. When
    /// composing externally via `compose`, the caller supplies pre-computed
    /// reports and these fields are unused.
    pub churn: ChurnObserver,
    pub complexity: ComplexityObserver,
}

impl HotspotObserver {
    #[must_use]
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            enabled: cfg.metrics.hotspot.enabled,
            weights: HotspotWeights {
                churn: cfg.metrics.hotspot.weight_churn,
                complexity: cfg.metrics.hotspot.weight_complexity,
            },
            churn: ChurnObserver::from_config(cfg),
            complexity: ComplexityObserver::from_config(cfg),
        }
    }

    #[must_use]
    pub fn scan(&self, root: &Path) -> HotspotReport {
        if !self.enabled {
            return HotspotReport::default();
        }
        let churn = self.churn.scan(root);
        let complexity = self.complexity.scan(root);
        compose(&churn, &complexity, None, None, self.weights)
    }
}

/// Per-file boost cap. The base boost is `1.0 + raw_factor`, where
/// `raw_factor` is the doc-drift contribution (commits since paired
/// doc) plus the coverage gap (`1 - coverage_ratio`). The combined
/// boost is clamped here so a doubly-bad file (stale doc AND zero
/// coverage) caps at the same multiplier as a singly-bad one — the
/// signal is "this file is rough", not "this file is two kinds of
/// rough", and we don't want hotspot ordering to degenerate into
/// ranking-by-feature-count.
const DOC_DRIFT_BOOST_MAX: f64 = 1.5;

/// Pure composer: zip a `ChurnReport` and `ComplexityReport` by file path
/// and emit a per-file score. Files appearing in only one of the two
/// inputs get a score of 0 and are filtered out.
///
/// `doc_freshness` and `coverage` are optional decorators. When
/// supplied, files whose paired doc is stale or whose line coverage
/// gaps the literature anchor receive a multiplicative score boost
/// shared at the cap [`DOC_DRIFT_BOOST_MAX`]. The reasoning follows
/// `documentation-quality-reference.md` §2.3 and
/// `test-quality-reference.md` §2.3: a hotspot whose doc no longer
/// describes the code, or whose tests don't exercise it, is doubly
/// costly to a reader, so it belongs higher in the drain queue than
/// a hotspot with fresh docs and full coverage.
#[must_use]
pub fn compose(
    churn: &ChurnReport,
    complexity: &ComplexityReport,
    doc_freshness: Option<&DocFreshnessReport>,
    coverage: Option<&CoverageReport>,
    weights: HotspotWeights,
) -> HotspotReport {
    let mut churn_by_path: BTreeMap<PathBuf, &FileChurn> = BTreeMap::new();
    for f in &churn.files {
        churn_by_path.insert(f.path.clone(), f);
    }
    let mut complexity_by_path: BTreeMap<PathBuf, &FileComplexity> = BTreeMap::new();
    for f in &complexity.files {
        complexity_by_path.insert(f.path.clone(), f);
    }

    // Build a per-src-file drift boost lookup. A pair entry whose src
    // contributes to several hotspot rows (e.g. one doc covers `src/a.rs`
    // and `src/b.rs`) lifts each of them by the same factor.
    let mut drift_boost: BTreeMap<PathBuf, f64> = BTreeMap::new();
    if let Some(freshness) = doc_freshness {
        // The report doesn't carry the user's `critical_commits` floor,
        // so the boost normalises against the default — a tightened
        // floor still saturates at `DOC_DRIFT_BOOST_MAX`, just sooner.
        let denom = f64::from(DocFreshnessConfig::DEFAULT_CRITICAL_COMMITS);
        for entry in &freshness.entries {
            if entry.src_commits_since_doc == 0 {
                continue;
            }
            let raw = f64::from(entry.src_commits_since_doc) / denom;
            let boost = 1.0 + raw.min(DOC_DRIFT_BOOST_MAX - 1.0);
            for src in &entry.src_paths {
                let entry = drift_boost.entry(src.clone()).or_insert(1.0);
                if boost > *entry {
                    *entry = boost;
                }
            }
        }
    }

    // Per-src-file uncovered boost. Same shape as the doc-drift
    // boost: `1.0 + (1 - coverage_ratio)`, capped so the combined
    // doc + coverage boost stays at `DOC_DRIFT_BOOST_MAX`.
    let mut uncovered_boost: BTreeMap<PathBuf, f64> = BTreeMap::new();
    if let Some(cov) = coverage {
        for entry in &cov.entries {
            let ratio = entry.line_coverage_pct / 100.0;
            if ratio >= 1.0 {
                continue;
            }
            let boost = 1.0 + (1.0 - ratio).min(DOC_DRIFT_BOOST_MAX - 1.0);
            uncovered_boost.insert(entry.path.clone(), boost);
        }
    }

    let mut entries: Vec<HotspotEntry> = Vec::new();
    for (path, complexity_file) in &complexity_by_path {
        let Some(churn_file) = churn_by_path.get(path) else {
            continue;
        };
        let ccn_sum: u32 = complexity_file.functions.iter().map(|f| f.ccn).sum::<u32>();
        let commits = churn_file.commits;
        if ccn_sum == 0 || commits == 0 {
            continue;
        }
        let base = weighted_score(commits, ccn_sum, weights);
        let doc_b = drift_boost.get(path).copied().unwrap_or(1.0);
        let cov_b = uncovered_boost.get(path).copied().unwrap_or(1.0);
        // Compose multiplicatively but share the cap: a file that's
        // both stale-docs and uncovered tops out at the same
        // multiplier as a singly-bad one. The signal is "rough", not
        // "rough on N axes".
        let combined = (doc_b * cov_b).min(DOC_DRIFT_BOOST_MAX);
        let score = base * combined;
        entries.push(HotspotEntry {
            path: path.clone(),
            ccn_sum,
            churn_commits: commits,
            score,
        });
    }

    entries.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });

    let max_score = entries.first().map_or(0.0, |e| e.score);
    let totals = HotspotTotals {
        files: entries.len(),
        max_score,
    };
    HotspotReport { entries, totals }
}

fn weighted_score(commits: u32, ccn_sum: u32, weights: HotspotWeights) -> f64 {
    let churn_term = weights.churn * f64::from(commits);
    let cmp_term = weights.complexity * f64::from(ccn_sum);
    churn_term * cmp_term
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HotspotReport {
    pub entries: Vec<HotspotEntry>,
    pub totals: HotspotTotals,
}

impl HotspotReport {
    #[must_use]
    pub fn worst_n(&self, n: usize) -> Vec<HotspotEntry> {
        let mut top = self.entries.clone();
        top.truncate(n);
        top
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HotspotEntry {
    pub path: PathBuf,
    pub ccn_sum: u32,
    pub churn_commits: u32,
    pub score: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HotspotTotals {
    pub files: usize,
    pub max_score: f64,
}

impl Observer for HotspotObserver {
    type Output = HotspotReport;

    fn meta(&self) -> ObservationMeta {
        ObservationMeta {
            name: "hotspot",
            version: 1,
        }
    }

    fn observe(&self, project_root: &Path) -> anyhow::Result<Self::Output> {
        Ok(self.scan(project_root))
    }
}

impl IntoFindings for HotspotReport {
    /// `hotspot` flag stays `false`; Calibration's percentile pass
    /// (TODO §Hotspot) toggles it on the top 10%.
    fn into_findings(&self) -> Vec<Finding> {
        self.entries
            .iter()
            .map(|entry| {
                Finding::new(
                    "hotspot",
                    Location::file(entry.path.clone()),
                    format!(
                        "hotspot score={:.0} (ccn_sum={}, churn={})",
                        entry.score, entry.ccn_sum, entry.churn_commits
                    ),
                    "",
                )
            })
            .collect()
    }
}

pub struct HotspotFeature;

impl Feature for HotspotFeature {
    fn meta(&self) -> FeatureMeta {
        FeatureMeta {
            name: "hotspot",
            version: 1,
            kind: FeatureKind::Observer,
        }
    }
    fn enabled(&self, cfg: &Config) -> bool {
        cfg.metrics.hotspot.enabled
    }
    fn lower(
        &self,
        reports: &ObserverReports,
        _cfg: &Config,
        _cal: &crate::core::calibration::Calibration,
        hotspot: &HotspotIndex,
    ) -> Vec<Finding> {
        let Some(h) = reports.hotspot.as_ref() else {
            return Vec::new();
        };
        h.into_findings()
            .into_iter()
            .map(|f| decorate(f, Severity::Ok, hotspot))
            .collect()
    }
}
