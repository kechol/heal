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

use ignore::gitignore::Gitignore;
use serde::{Deserialize, Serialize};

use crate::core::config::Config;
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::core::severity::Severity;
use crate::feature::{decorate, Family, Feature, FeatureKind, FeatureMeta, HotspotIndex};
use crate::observer::docs::corpus::{read_doc_bodies, DocBody};
use crate::observer::docs::markdown::{
    extract_links, is_external, resolve_relative, split_link_target,
};
use crate::observer::docs::walk::{build_matcher, is_match};

pub struct OrphanPagesObserver {
    enabled: bool,
    /// Pre-read Layer B docs whose bodies the observer scans for
    /// outgoing links. Caller (`run_all`) reads these once and shares
    /// the slices with link-health and todo-density.
    standalone: Vec<DocBody>,
    /// Layer A pair docs — only paths are needed (they're seeded as
    /// "linked" via the pair entry, no body scan).
    paired_docs: Vec<PathBuf>,
    /// Compiled `[features.docs.standalone].entrypoints` matcher. Pages
    /// matching this set count as reachable even when no other doc
    /// links to them — covers Starlight / Hugo / Docusaurus sidebar
    /// configs the observer can't read directly.
    entrypoints: Option<Gitignore>,
}

impl OrphanPagesObserver {
    #[must_use]
    pub fn from_inputs(cfg: &Config, standalone: Vec<DocBody>, paired_docs: Vec<PathBuf>) -> Self {
        Self {
            enabled: cfg.features.docs.enabled,
            standalone,
            paired_docs,
            entrypoints: build_matcher(Path::new(""), &cfg.features.docs.standalone.entrypoints),
        }
    }

    fn matches_entrypoint(&self, doc: &Path) -> bool {
        self.entrypoints.as_ref().is_some_and(|m| is_match(m, doc))
    }

    /// Convenience for tests / out-of-band callers: read each Layer B
    /// path off disk before constructing. Production runs go through
    /// the shared corpus in `observers::run_all` and use
    /// [`Self::from_inputs`] directly.
    #[must_use]
    pub fn from_paths(
        cfg: &Config,
        root: &Path,
        standalone: &[PathBuf],
        paired_docs: Vec<PathBuf>,
    ) -> Self {
        Self::from_inputs(cfg, read_doc_bodies(root, standalone), paired_docs)
    }

    #[must_use]
    pub fn scan(&self) -> OrphanPagesReport {
        let mut report = OrphanPagesReport::default();
        if !self.enabled || self.standalone.is_empty() {
            return report;
        }
        let mut linked: HashSet<PathBuf> = HashSet::new();
        // Pre-seed with paired docs — each is reachable through its
        // pair entry, so the SSoT counts as a link target.
        for paired in &self.paired_docs {
            linked.insert(paired.clone());
        }
        // Conventional entry points are never orphans even when
        // nothing else links to them. `README.md` / `index.{md,mdx,rst}`
        // at any depth cover Markdown / Starlight / Docusaurus
        // landing pages. The configured `entrypoints` globs add
        // SSG-specific reachability (sidebar / nav configs the
        // observer can't read directly).
        for doc in &self.standalone {
            if is_entry_point(&doc.path) || self.matches_entrypoint(&doc.path) {
                linked.insert(doc.path.clone());
            }
        }
        for doc in &self.standalone {
            for link in extract_links(&doc.body) {
                if is_external(&link.target) {
                    continue;
                }
                let (path, _anchor) = split_link_target(&link.target);
                if path.is_empty() {
                    continue;
                }
                linked.insert(resolve_relative(&doc.path, path));
            }
        }
        let orphans: Vec<PathBuf> = self
            .standalone
            .iter()
            .filter(|d| !linked.contains(&d.path))
            .map(|d| d.path.clone())
            .collect();
        report.totals = OrphanPagesTotals {
            scanned_docs: self.standalone.len(),
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

/// `README.md` / `index.{md,mdx,rst}` (any depth) are never orphans —
/// reachability comes from outside the doc graph (GitHub repo home,
/// Starlight / mdBook home, `cargo doc` index, MDX-based SSGs).
fn is_entry_point(doc: &Path) -> bool {
    let Some(name) = doc.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "readme.md" | "readme.mdx" | "readme.rst" | "index.md" | "index.mdx" | "index.rst"
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
