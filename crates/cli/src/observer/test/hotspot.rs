//! `test_hotspot` — per-src-file `commits × uncov_pct` composite.
//!
//! The test-family analogue of code Hotspot. Same Tornhill structure
//! (`volatility × cost-to-get-right`), but the cost factor is the
//! coverage gap rather than the CCN sum: a file the team keeps editing
//! while large slices of it stay unverified is where unverified change
//! is piling up fastest, and that's the right "where do we add tests"
//! prioritisation signal — independent of how complex the code is.
//!
//! `compose` is a pure function over already-computed reports so
//! `heal status` can reuse the work the Churn and Coverage observers
//! already did. Files absent from the lcov payload but present in
//! `ChurnReport` are treated as 100% gap (= untested) — this is the
//! whole point: lcov reporters routinely omit zero-coverage files,
//! and "actively edited and never tested" is exactly the case the
//! metric exists to surface.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::core::config::Config;
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::core::severity::Severity;
use crate::feature::{decorate, Family, Feature, FeatureKind, FeatureMeta, HotspotIndex};
use crate::observer::code::churn::ChurnReport;
use crate::observer::shared::lang::Language;
use crate::observer::test::coverage::CoverageReport;
use crate::observers::ObserverReports;

#[derive(Debug, Clone, Default)]
pub struct TestHotspotObserver {
    pub enabled: bool,
}

impl TestHotspotObserver {
    #[must_use]
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            enabled: cfg.features.test.enabled && cfg.features.test.coverage.enabled,
        }
    }
}

/// Pure composer over already-computed `ChurnReport` and
/// `CoverageReport`. Universe = the union of both, restricted to
/// paths whose extension `Language::from_path` recognises (so doc /
/// asset / lockfile churn rows can't pollute the ranking even when
/// they fail to appear in lcov).
///
/// Files that pass the universe filter but appear in neither report,
/// or whose `commits = 0`, score zero and are dropped. Files at
/// `coverage = 100%` likewise score zero (gap = 0) and are dropped.
#[must_use]
pub fn compose(churn: &ChurnReport, coverage: Option<&CoverageReport>) -> TestHotspotReport {
    let mut churn_by_path: BTreeMap<PathBuf, u32> = BTreeMap::new();
    for f in &churn.files {
        if Language::from_path(&f.path).is_some() {
            churn_by_path.insert(f.path.clone(), f.commits);
        }
    }
    let mut gap_by_path: BTreeMap<PathBuf, f64> = BTreeMap::new();
    if let Some(cov) = coverage {
        for entry in &cov.entries {
            if Language::from_path(&entry.path).is_none() {
                continue;
            }
            let gap = (100.0 - entry.line_coverage_pct).max(0.0);
            gap_by_path.insert(entry.path.clone(), gap);
        }
    }

    let mut entries: Vec<TestHotspotEntry> = Vec::new();
    let mut universe: Vec<PathBuf> = churn_by_path.keys().cloned().collect();
    for path in gap_by_path.keys() {
        if !churn_by_path.contains_key(path) {
            universe.push(path.clone());
        }
    }

    for path in &universe {
        let commits = churn_by_path.get(path).copied().unwrap_or(0);
        if commits == 0 {
            continue;
        }
        // Coverage absence => assume untested. This is load-bearing:
        // many lcov reporters omit zero-coverage files entirely, and
        // the "edited a lot, never tested" case is the metric's
        // single most important target.
        let gap = gap_by_path.get(path).copied().unwrap_or(100.0);
        if gap <= 0.0 {
            continue;
        }
        let score = f64::from(commits) * gap;
        entries.push(TestHotspotEntry {
            path: path.clone(),
            churn_commits: commits,
            uncov_pct: gap,
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
    TestHotspotReport {
        totals: TestHotspotTotals {
            files: entries.len(),
            max_score,
        },
        entries,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TestHotspotReport {
    pub entries: Vec<TestHotspotEntry>,
    pub totals: TestHotspotTotals,
}

impl TestHotspotReport {
    #[must_use]
    pub fn worst_n(&self, n: usize) -> Vec<TestHotspotEntry> {
        let mut top = self.entries.clone();
        top.truncate(n);
        top
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TestHotspotEntry {
    pub path: PathBuf,
    pub churn_commits: u32,
    pub uncov_pct: f64,
    pub score: f64,
}

impl Eq for TestHotspotEntry {}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TestHotspotTotals {
    pub files: usize,
    pub max_score: f64,
}

impl Eq for TestHotspotTotals {}

impl IntoFindings for TestHotspotReport {
    fn into_findings(&self) -> Vec<Finding> {
        self.entries
            .iter()
            .map(|entry| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let gap_int = entry.uncov_pct.round() as u32;
                Finding::new(
                    Finding::METRIC_TEST_HOTSPOT,
                    Location::file(entry.path.clone()),
                    format!(
                        "test-hotspot score={:.0} (uncov={}%, churn={})",
                        entry.score, gap_int, entry.churn_commits
                    ),
                    "",
                )
            })
            .collect()
    }
}

pub struct TestHotspotFeature;

impl Feature for TestHotspotFeature {
    fn meta(&self) -> FeatureMeta {
        FeatureMeta {
            name: Finding::METRIC_TEST_HOTSPOT,
            version: 1,
            kind: FeatureKind::Observer,
        }
    }
    fn enabled(&self, cfg: &Config) -> bool {
        cfg.features.test.enabled && cfg.features.test.coverage.enabled
    }
    fn family(&self) -> Family {
        Family::Test
    }
    fn lower(
        &self,
        reports: &ObserverReports,
        _cfg: &Config,
        _cal: &crate::core::calibration::Calibration,
        hotspot: &HotspotIndex,
    ) -> Vec<Finding> {
        let Some(h) = reports.test_hotspot.as_ref() else {
            return Vec::new();
        };
        h.into_findings()
            .into_iter()
            .map(|f| decorate(f, Severity::Ok, hotspot))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observer::code::churn::{ChurnReport, ChurnTotals, FileChurn};
    #[cfg(feature = "lang-rust")]
    use crate::observer::test::coverage::{CoverageEntry, CoverageReport};

    fn churn_of(items: &[(&str, u32)]) -> ChurnReport {
        ChurnReport {
            files: items
                .iter()
                .map(|(p, c)| FileChurn {
                    path: PathBuf::from(p),
                    commits: *c,
                    lines_added: 0,
                    lines_deleted: 0,
                })
                .collect(),
            totals: ChurnTotals::default(),
            since_days: 90,
        }
    }

    // Test helper consumed only by the `lang-rust` cases below.
    // Gating keeps single-language CI builds (e.g.
    // `--features lang-javascript`) from tripping `-D dead-code`.
    #[cfg(feature = "lang-rust")]
    fn cov_of(items: &[(&str, f64)]) -> CoverageReport {
        CoverageReport {
            source: None,
            entries: items
                .iter()
                .map(|(p, pct)| CoverageEntry {
                    path: PathBuf::from(p),
                    lines_found: 100,
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    lines_hit: pct.round() as u32,
                    branches_found: 0,
                    branches_hit: 0,
                    line_coverage_pct: *pct,
                })
                .collect(),
        }
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn churn_with_no_coverage_entry_treated_as_fully_uncovered() {
        // src/orphan.rs is touched but lcov never mentioned it — the
        // exact failure mode the metric exists to surface.
        let churn = churn_of(&[("src/orphan.rs", 5), ("src/tested.rs", 5)]);
        let cov = cov_of(&[("src/tested.rs", 80.0)]);
        let report = compose(&churn, Some(&cov));
        // orphan: commits=5 × gap=100 = 500
        // tested: commits=5 × gap=20  = 100
        // orphan should rank above tested.
        assert_eq!(report.entries[0].path.to_string_lossy(), "src/orphan.rs");
        assert!((report.entries[0].score - 500.0).abs() < f64::EPSILON);
        assert_eq!(report.entries[1].path.to_string_lossy(), "src/tested.rs");
        assert!((report.entries[1].score - 100.0).abs() < f64::EPSILON);
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn fully_covered_file_drops_out() {
        let churn = churn_of(&[("src/full.rs", 5)]);
        let cov = cov_of(&[("src/full.rs", 100.0)]);
        let report = compose(&churn, Some(&cov));
        assert!(report.entries.is_empty());
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn zero_churn_file_drops_out_even_when_uncovered() {
        let churn = churn_of(&[("src/cold.rs", 0)]);
        let cov = cov_of(&[("src/cold.rs", 0.0)]);
        let report = compose(&churn, Some(&cov));
        assert!(report.entries.is_empty());
    }

    #[test]
    fn non_src_extensions_excluded_from_universe() {
        // Markdown, lock files, JSON shouldn't enter the test_hotspot
        // ranking even when they show up in churn.
        let churn = churn_of(&[("README.md", 10), ("Cargo.lock", 8)]);
        let report = compose(&churn, None);
        assert!(report.entries.is_empty());
    }

    #[test]
    fn empty_when_no_coverage_and_no_churn() {
        let churn = churn_of(&[]);
        let report = compose(&churn, None);
        assert!(report.entries.is_empty());
    }
}
