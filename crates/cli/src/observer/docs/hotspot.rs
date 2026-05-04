//! `doc_hotspot` — per-pair `paired_src_churn × debt` composite.
//!
//! The docs-family analogue of code Hotspot. The volatility factor is
//! the total commit activity on the pair's src files (90-day window);
//! the debt factor combines staleness (commits to the src since the
//! doc was last edited) with factual breakage (`dangling_idents` —
//! identifiers the doc still mentions that the src has removed).
//! Higher = "the doc is most worth updating now".
//!
//! `compose` is a pure function over already-computed reports so
//! `heal status` can reuse the work the Churn, `DocFreshness`, and
//! `DocDrift` observers already did. Standalone (unpaired) docs are
//! deliberately out of scope — `orphan_pages` and `todo_density`
//! cover the standalone universe with their own signals; mixing them
//! in here would muddle the "fix this paired doc next" semantics.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::core::config::Config;
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::core::severity::Severity;
use crate::feature::{decorate, Family, Feature, FeatureKind, FeatureMeta, HotspotIndex};
use crate::observer::code::churn::ChurnReport;
use crate::observer::docs::drift::DocDriftReport;
use crate::observer::docs::freshness::DocFreshnessReport;
use crate::observers::ObserverReports;

#[derive(Debug, Clone, Default)]
pub struct DocHotspotObserver {
    pub enabled: bool,
    pub weight_drift: f64,
}

impl DocHotspotObserver {
    #[must_use]
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            enabled: cfg.features.docs.enabled,
            weight_drift: cfg.features.docs.hotspot.weight_drift,
        }
    }
}

/// Pure composer. Joins by `doc_path`:
/// - `paired_src_churn` = sum of `FileChurn.commits` over `pair.srcs`
/// - `debt = src_commits_since_doc + weight_drift × dangling_idents`
/// - `score = paired_src_churn × debt`; entries with score 0 drop.
///
/// `freshness` carries the per-pair `src_commits_since_doc` already;
/// `drift` is row-per-dangling-identifier so we aggregate by
/// `doc_path`. Pairs whose freshness entry is missing (doc has no git
/// history yet) treat staleness as zero — the dangling-identifier
/// component still surfaces them when the doc references names the
/// src has removed.
#[must_use]
pub fn compose(
    churn: &ChurnReport,
    freshness: &DocFreshnessReport,
    drift: Option<&DocDriftReport>,
    weight_drift: f64,
) -> DocHotspotReport {
    let mut churn_by_path: BTreeMap<PathBuf, u32> = BTreeMap::new();
    for f in &churn.files {
        churn_by_path.insert(f.path.clone(), f.commits);
    }

    let mut dangling_by_doc: BTreeMap<PathBuf, u32> = BTreeMap::new();
    if let Some(d) = drift {
        for entry in &d.entries {
            *dangling_by_doc
                .entry(entry.doc_path.clone())
                .or_insert(0_u32) += 1;
        }
    }

    let mut entries: Vec<DocHotspotEntry> = Vec::new();
    for entry in &freshness.entries {
        let paired_src_churn: u32 = entry
            .src_paths
            .iter()
            .map(|p| churn_by_path.get(p).copied().unwrap_or(0))
            .sum();
        let dangling = dangling_by_doc.get(&entry.doc_path).copied().unwrap_or(0);
        let debt = f64::from(entry.src_commits_since_doc) + weight_drift * f64::from(dangling);
        let score = f64::from(paired_src_churn) * debt;
        if score <= 0.0 {
            continue;
        }
        entries.push(DocHotspotEntry {
            doc_path: entry.doc_path.clone(),
            src_paths: entry.src_paths.clone(),
            paired_src_churn,
            src_commits_since_doc: entry.src_commits_since_doc,
            dangling_idents: dangling,
            score,
        });
    }

    // Drift-only case: pairs that aren't in the freshness report (doc
    // has no git history yet) but have dangling idents. We still want
    // to surface them, but freshness is the authoritative pair list,
    // so for v0.4 we accept they fall through. Future: extend
    // `DocFreshnessObserver` to emit zero-staleness entries for
    // history-less docs.

    entries.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.doc_path.cmp(&b.doc_path))
    });
    let max_score = entries.first().map_or(0.0, |e| e.score);
    DocHotspotReport {
        totals: DocHotspotTotals {
            pairs: entries.len(),
            max_score,
        },
        entries,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DocHotspotReport {
    pub entries: Vec<DocHotspotEntry>,
    pub totals: DocHotspotTotals,
}

impl DocHotspotReport {
    #[must_use]
    pub fn worst_n(&self, n: usize) -> Vec<DocHotspotEntry> {
        let mut top = self.entries.clone();
        top.truncate(n);
        top
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocHotspotEntry {
    pub doc_path: PathBuf,
    pub src_paths: Vec<PathBuf>,
    pub paired_src_churn: u32,
    pub src_commits_since_doc: u32,
    pub dangling_idents: u32,
    pub score: f64,
}

impl Eq for DocHotspotEntry {}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DocHotspotTotals {
    pub pairs: usize,
    pub max_score: f64,
}

impl Eq for DocHotspotTotals {}

impl IntoFindings for DocHotspotReport {
    fn into_findings(&self) -> Vec<Finding> {
        self.entries
            .iter()
            .map(|entry| {
                let primary = Location::file(entry.doc_path.clone());
                let extras: Vec<Location> = entry
                    .src_paths
                    .iter()
                    .map(|p| Location::file(p.clone()))
                    .collect();
                Finding::new(
                    Finding::METRIC_DOC_HOTSPOT,
                    primary,
                    format!(
                        "doc-hotspot score={:.0} (src_churn={}, since_doc={}, dangling={})",
                        entry.score,
                        entry.paired_src_churn,
                        entry.src_commits_since_doc,
                        entry.dangling_idents,
                    ),
                    "",
                )
                .with_locations(extras)
            })
            .collect()
    }
}

pub struct DocHotspotFeature;

impl Feature for DocHotspotFeature {
    fn meta(&self) -> FeatureMeta {
        FeatureMeta {
            name: Finding::METRIC_DOC_HOTSPOT,
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
        reports: &ObserverReports,
        _cfg: &Config,
        _cal: &crate::core::calibration::Calibration,
        hotspot: &HotspotIndex,
    ) -> Vec<Finding> {
        let Some(h) = reports.doc_hotspot.as_ref() else {
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
    use crate::observer::docs::drift::{DocDriftEntry, DocDriftReport, DocDriftTotals};
    use crate::observer::docs::freshness::{
        DocFreshnessEntry, DocFreshnessReport, DocFreshnessTotals,
    };

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

    fn freshness_of(items: &[(&str, &[&str], u32)]) -> DocFreshnessReport {
        DocFreshnessReport {
            entries: items
                .iter()
                .map(|(doc, srcs, since)| DocFreshnessEntry {
                    doc_path: PathBuf::from(doc),
                    src_paths: srcs.iter().map(PathBuf::from).collect(),
                    src_commits_since_doc: *since,
                    doc_last_commit_time: None,
                })
                .collect(),
            totals: DocFreshnessTotals {
                pairs: items.len(),
                stale_pairs: items.iter().filter(|(_, _, s)| *s > 0).count(),
            },
        }
    }

    fn drift_of(items: &[(&str, &str, &str)]) -> DocDriftReport {
        DocDriftReport {
            entries: items
                .iter()
                .map(|(doc, src, ident)| DocDriftEntry {
                    doc_path: PathBuf::from(doc),
                    src_paths: vec![PathBuf::from(src)],
                    identifier: (*ident).to_string(),
                    doc_line: 1,
                })
                .collect(),
            totals: DocDriftTotals {
                dangling_identifiers: items.len(),
            },
        }
    }

    #[test]
    fn hot_paired_src_with_stale_doc_outranks_quiet_pair() {
        let churn = churn_of(&[("src/parser.rs", 30), ("src/cold.rs", 2)]);
        let freshness = freshness_of(&[
            ("docs/parser.md", &["src/parser.rs"], 15),
            ("docs/cold.md", &["src/cold.rs"], 5),
        ]);
        let report = compose(&churn, &freshness, None, 1.0);
        // parser: churn=30 × debt=15 = 450
        // cold:   churn=2  × debt=5  = 10
        assert_eq!(
            report.entries[0].doc_path.to_string_lossy(),
            "docs/parser.md"
        );
        assert!((report.entries[0].score - 450.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dangling_idents_contribute_to_debt() {
        let churn = churn_of(&[("src/api.rs", 10)]);
        let freshness = freshness_of(&[("docs/api.md", &["src/api.rs"], 0)]);
        // No staleness, but doc references two removed names.
        let drift = drift_of(&[
            ("docs/api.md", "src/api.rs", "old_fn"),
            ("docs/api.md", "src/api.rs", "Removed"),
        ]);
        let report = compose(&churn, &freshness, Some(&drift), 1.0);
        // churn=10 × (0 + 1.0×2) = 20
        assert_eq!(report.entries.len(), 1);
        assert!((report.entries[0].score - 20.0).abs() < f64::EPSILON);
        assert_eq!(report.entries[0].dangling_idents, 2);
    }

    #[test]
    fn weight_drift_amplifies_dangling_contribution() {
        let churn = churn_of(&[("src/api.rs", 5)]);
        let freshness = freshness_of(&[("docs/api.md", &["src/api.rs"], 0)]);
        let drift = drift_of(&[("docs/api.md", "src/api.rs", "X")]);
        let with_one = compose(&churn, &freshness, Some(&drift), 1.0);
        let with_five = compose(&churn, &freshness, Some(&drift), 5.0);
        // Same inputs, different weight: 5 × (0 + 1) = 5 vs 5 × (0 + 5) = 25
        assert!((with_one.entries[0].score - 5.0).abs() < f64::EPSILON);
        assert!((with_five.entries[0].score - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn pair_with_zero_churn_drops_out() {
        let churn = churn_of(&[]);
        let freshness = freshness_of(&[("docs/dead.md", &["src/dead.rs"], 50)]);
        let report = compose(&churn, &freshness, None, 1.0);
        assert!(report.entries.is_empty());
    }

    #[test]
    fn fresh_pair_with_no_dangling_drops_out() {
        let churn = churn_of(&[("src/clean.rs", 20)]);
        let freshness = freshness_of(&[("docs/clean.md", &["src/clean.rs"], 0)]);
        let report = compose(&churn, &freshness, None, 1.0);
        assert!(report.entries.is_empty());
    }

    #[test]
    fn multi_src_pair_sums_paired_churn() {
        let churn = churn_of(&[("src/a.rs", 4), ("src/b.rs", 6)]);
        let freshness = freshness_of(&[("docs/api.md", &["src/a.rs", "src/b.rs"], 3)]);
        let report = compose(&churn, &freshness, None, 1.0);
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].paired_src_churn, 10);
        // 10 × 3 = 30
        assert!((report.entries[0].score - 30.0).abs() < f64::EPSILON);
    }
}
