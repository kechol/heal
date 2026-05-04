//! Doc coverage (initial pass): surface src files in `.heal/doc_pairs.json`
//! whose paired `doc` is missing from disk.
//!
//! ## Why Medium, not Critical
//!
//! Per `documentation-quality-reference.md` §5.1 ("Coverage-driven
//! trap"): pushing coverage to 100 % rewards generating empty
//! docstrings. Surfacing missing docs at Medium keeps the signal
//! actionable without inviting the agent to manufacture filler. Users
//! who genuinely want stricter coverage can override via
//! `[policy.drain.metrics.doc_coverage]`.
//!
//! ## Initial scope
//!
//! This pass only handles **explicitly mapped** src⇔doc pairs that the
//! `/heal-doc-pair-setup` skill has captured in
//! `.heal/doc_pairs.json`. A future iteration walks every public API
//! and looks for matching doc anchors; that requires a per-language
//! "what is public" rule and is deferred.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::config::Config;
use crate::core::doc_pairs::DocPair;
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::core::severity::Severity;
use crate::feature::{decorate, Family, Feature, FeatureKind, FeatureMeta, HotspotIndex};

#[derive(Debug, Clone, Default)]
pub struct DocCoverageObserver {
    pub enabled: bool,
    pub pairs: Vec<DocPair>,
}

impl DocCoverageObserver {
    #[must_use]
    pub fn from_config_and_pairs(cfg: &Config, pairs: Vec<DocPair>) -> Self {
        Self {
            enabled: cfg.features.docs.enabled,
            pairs,
        }
    }

    #[must_use]
    pub fn scan(&self, root: &Path) -> DocCoverageReport {
        let mut report = DocCoverageReport::default();
        if !self.enabled || self.pairs.is_empty() {
            return report;
        }
        for pair in &self.pairs {
            let doc_present = root.join(&pair.doc).exists();
            // A pair entry can refer to multiple srcs — skip srcs the
            // user has already deleted, since they're surfaced through
            // the integrity warning at load time.
            for src in &pair.srcs {
                if !root.join(src).exists() {
                    continue;
                }
                if !doc_present {
                    report.missing.push(DocCoverageEntry {
                        src_path: PathBuf::from(src),
                        expected_doc_path: PathBuf::from(&pair.doc),
                    });
                }
            }
        }
        report.missing.sort_by(|a, b| a.src_path.cmp(&b.src_path));
        report.totals = DocCoverageTotals {
            tracked_srcs: self.pairs.iter().map(|p| p.srcs.len()).sum(),
            missing_docs: report.missing.len(),
        };
        report
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocCoverageReport {
    pub missing: Vec<DocCoverageEntry>,
    pub totals: DocCoverageTotals,
}

impl DocCoverageReport {
    #[must_use]
    pub fn worst_n(&self, n: usize) -> Vec<DocCoverageEntry> {
        let mut top = self.missing.clone();
        top.truncate(n);
        top
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocCoverageEntry {
    pub src_path: PathBuf,
    pub expected_doc_path: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocCoverageTotals {
    pub tracked_srcs: usize,
    pub missing_docs: usize,
}

impl IntoFindings for DocCoverageReport {
    fn into_findings(&self) -> Vec<Finding> {
        self.missing
            .iter()
            .map(|entry| {
                let primary = Location::file(entry.src_path.clone());
                let summary = format!(
                    "doc_coverage: paired doc `{}` is missing",
                    entry.expected_doc_path.display(),
                );
                let seed = format!("doc_coverage:{}", entry.src_path.to_string_lossy());
                Finding::new("doc_coverage", primary, summary, &seed)
                    .with_locations(vec![Location::file(entry.expected_doc_path.clone())])
            })
            .collect()
    }
}

pub struct DocCoverageFeature;

impl Feature for DocCoverageFeature {
    fn meta(&self) -> FeatureMeta {
        FeatureMeta {
            name: "doc_coverage",
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
        _cfg: &Config,
        _cal: &crate::core::calibration::Calibration,
        hotspot: &HotspotIndex,
    ) -> Vec<Finding> {
        let Some(report) = reports.doc_coverage.as_ref() else {
            return Vec::new();
        };
        // Severity stays Medium per the §5.1 trap discussion. Per-team
        // overrides go through `[policy.drain.metrics.doc_coverage]`.
        report
            .into_findings()
            .into_iter()
            .map(|f| decorate(f, Severity::Medium, hotspot))
            .collect()
    }
}
