//! Orphan pages — Layer B docs that no other doc references.
//!
//! Per `documentation-quality-reference.md` §5.5 ("doc bloat trap"),
//! adding observers without a deletion side rewards write-only growth.
//! Orphan detection is the deletion-side counterpart to coverage: a
//! page nobody links to is either dead weight, badly indexed, or both.
//!
//! The observer is filesystem-only (no network) and matches Layer B's
//! discovery rules. Layer A `doc` paths in `.heal/doc_pairs.json` are
//! considered "linked" by the pair itself — they never count as
//! orphans even when the standalone include glob would otherwise catch
//! them.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::config::Config;
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::core::severity::Severity;
use crate::feature::{decorate, Feature, FeatureKind, FeatureMeta, HotspotIndex};
use crate::observer::doc_markdown::{
    extract_links, is_external, resolve_relative, split_link_target,
};

pub struct OrphanPagesObserver {
    enabled: bool,
    standalone_docs: Vec<PathBuf>,
    paired_docs: Vec<PathBuf>,
}

impl OrphanPagesObserver {
    #[must_use]
    pub fn from_config_and_inputs(
        cfg: &Config,
        standalone_docs: Vec<PathBuf>,
        paired_docs: Vec<PathBuf>,
    ) -> Self {
        Self {
            enabled: cfg.features.docs.enabled,
            standalone_docs,
            paired_docs,
        }
    }

    #[must_use]
    pub fn scan(&self, root: &Path) -> OrphanPagesReport {
        let mut report = OrphanPagesReport::default();
        if !self.enabled || self.standalone_docs.is_empty() {
            return report;
        }
        let mut linked: HashSet<PathBuf> = HashSet::new();
        // Pre-seed with paired docs — each is reachable through its
        // pair entry, so the SSoT counts as a link target.
        for paired in &self.paired_docs {
            linked.insert(paired.clone());
        }
        // Conventional entry points are never orphans even when
        // nothing else links to them. `README.md` at any depth and
        // `index.md` at the standalone root cover the typical
        // Markdown / Starlight site layouts.
        for doc in &self.standalone_docs {
            if is_entry_point(doc) {
                linked.insert(doc.clone());
            }
        }
        for doc in &self.standalone_docs {
            let abs = root.join(doc);
            let Ok(body) = std::fs::read_to_string(&abs) else {
                continue;
            };
            for link in extract_links(&body) {
                if is_external(&link.target) {
                    continue;
                }
                let (path, _anchor) = split_link_target(&link.target);
                if path.is_empty() {
                    continue;
                }
                linked.insert(resolve_relative(doc, path));
            }
        }
        let orphans: Vec<PathBuf> = self
            .standalone_docs
            .iter()
            .filter(|p| !linked.contains(*p))
            .cloned()
            .collect();
        report.totals = OrphanPagesTotals {
            scanned_docs: self.standalone_docs.len(),
            orphans: orphans.len(),
        };
        report.orphans = orphans;
        report.orphans.sort();
        report
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrphanPagesReport {
    pub orphans: Vec<PathBuf>,
    pub totals: OrphanPagesTotals,
}

impl OrphanPagesReport {
    #[must_use]
    pub fn worst_n(&self, n: usize) -> Vec<PathBuf> {
        let mut top = self.orphans.clone();
        top.truncate(n);
        top
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrphanPagesTotals {
    pub scanned_docs: usize,
    pub orphans: usize,
}

/// `README.md` and `index.md` (any depth) are never orphans —
/// reachability comes from outside the doc graph (GitHub repo home,
/// Starlight / mdBook home, `cargo doc` index).
fn is_entry_point(doc: &Path) -> bool {
    let Some(name) = doc.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "readme.md" | "readme.rst" | "index.md" | "index.rst"
    )
}

impl IntoFindings for OrphanPagesReport {
    fn into_findings(&self) -> Vec<Finding> {
        self.orphans
            .iter()
            .map(|p| {
                let primary = Location::file(p.clone());
                let summary = "orphan_pages: no other doc links here".to_owned();
                let seed = format!("orphan_pages:{}", p.to_string_lossy());
                Finding::new("orphan_pages", primary, summary, &seed)
            })
            .collect()
    }
}

pub struct OrphanPagesFeature;

impl Feature for OrphanPagesFeature {
    fn meta(&self) -> FeatureMeta {
        FeatureMeta {
            name: "orphan_pages",
            version: 1,
            kind: FeatureKind::DocsScanner,
        }
    }
    fn enabled(&self, cfg: &Config) -> bool {
        cfg.features.docs.enabled
    }
    fn lower(
        &self,
        reports: &crate::observers::ObserverReports,
        _cfg: &Config,
        _cal: &crate::core::calibration::Calibration,
        hotspot: &HotspotIndex,
    ) -> Vec<Finding> {
        let Some(report) = reports.orphan_pages.as_ref() else {
            return Vec::new();
        };
        report
            .into_findings()
            .into_iter()
            .map(|f| decorate(f, Severity::Medium, hotspot))
            .collect()
    }
}
