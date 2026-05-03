//! TODO marker density per Layer A / B doc.
//!
//! Counts occurrences of `TODO`, `FIXME`, `XXX`, `TBD`, and the
//! Japanese `[要確認]` / `[要修正]` markers users routinely embed.
//! These markers are author-confessed incompleteness — they're cheap
//! to surface as a finding because the writer already flagged the
//! area; the cost is only the surfacing.
//!
//! Severity scales with marker count per file rather than per line —
//! a doc with 20 TODOs is a stronger signal than a 200-line doc with
//! one TODO buried at line 192. The floors live in
//! `[features.docs.todo_density]` (added in `DocsConfig`) for users
//! who want to tighten or loosen the gate.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::config::Config;
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::core::severity::Severity;
use crate::feature::{decorate, Feature, FeatureKind, FeatureMeta, HotspotIndex};
use crate::observer::doc_corpus::{read_doc_bodies, DocBody};

/// Markers we count. Single-pass over each line so adding a new marker
/// is a one-element edit.
const MARKERS: &[&str] = &["TODO", "FIXME", "XXX", "TBD", "[要確認]", "[要修正]"];

pub struct TodoDensityObserver {
    enabled: bool,
    docs: Vec<DocBody>,
}

/// Per-doc marker count below which the finding is informational
/// (`Severity::Ok`). Spec guidance: "1 page > 3 markers = Review."
pub(crate) const MEDIUM_THRESHOLD: u32 = 3;
/// Per-doc marker count at or above which the finding escalates to
/// `Severity::High`.
pub(crate) const HIGH_THRESHOLD: u32 = 10;

impl TodoDensityObserver {
    #[must_use]
    pub fn from_inputs(cfg: &Config, docs: Vec<DocBody>) -> Self {
        Self {
            enabled: cfg.features.docs.enabled,
            docs,
        }
    }

    /// Convenience constructor for tests: read each path off disk
    /// before constructing. Production runs share the corpus through
    /// [`Self::from_inputs`].
    #[must_use]
    pub fn from_paths(cfg: &Config, root: &Path, paths: &[PathBuf]) -> Self {
        Self::from_inputs(cfg, read_doc_bodies(root, paths))
    }

    #[must_use]
    pub fn scan(&self) -> TodoDensityReport {
        let mut report = TodoDensityReport::default();
        if !self.enabled || self.docs.is_empty() {
            return report;
        }
        let mut entries: Vec<TodoDensityEntry> = Vec::new();
        for doc in &self.docs {
            let count = count_markers(&doc.body);
            if count == 0 {
                continue;
            }
            entries.push(TodoDensityEntry {
                doc_path: doc.path.clone(),
                marker_count: count,
            });
        }
        entries.sort_by(|a, b| {
            b.marker_count
                .cmp(&a.marker_count)
                .then_with(|| a.doc_path.cmp(&b.doc_path))
        });
        report.totals = TodoDensityTotals {
            scanned_docs: self.docs.len(),
            docs_with_markers: entries.len(),
            total_markers: entries.iter().map(|e| u64::from(e.marker_count)).sum(),
        };
        report.entries = entries;
        report
    }
}

/// Map a per-doc marker count to a Severity using the package floors.
/// Free-function so the Feature lowering path doesn't need to
/// reconstruct an observer just to call it.
#[must_use]
pub fn classify(marker_count: u32) -> Severity {
    if marker_count >= HIGH_THRESHOLD {
        Severity::High
    } else if marker_count >= MEDIUM_THRESHOLD {
        Severity::Medium
    } else {
        // 0..MEDIUM_THRESHOLD: informational, not surfaced.
        Severity::Ok
    }
}

/// Count marker occurrences in `body`, skipping fenced code blocks (a
/// `TODO` inside an example shouldn't count as a doc marker).
fn count_markers(body: &str) -> u32 {
    let mut count: u32 = 0;
    for (_, line) in crate::observer::doc_markdown::iter_prose_lines(body) {
        for m in MARKERS {
            count = count.saturating_add(u32::try_from(line.matches(m).count()).unwrap_or(0));
        }
    }
    count
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoDensityReport {
    pub entries: Vec<TodoDensityEntry>,
    pub totals: TodoDensityTotals,
}

impl TodoDensityReport {
    #[must_use]
    pub fn worst_n(&self, n: usize) -> Vec<TodoDensityEntry> {
        let mut top = self.entries.clone();
        top.truncate(n);
        top
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoDensityEntry {
    pub doc_path: PathBuf,
    pub marker_count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoDensityTotals {
    pub scanned_docs: usize,
    pub docs_with_markers: usize,
    pub total_markers: u64,
}

impl IntoFindings for TodoDensityReport {
    fn into_findings(&self) -> Vec<Finding> {
        self.entries
            .iter()
            .map(|entry| {
                let primary = Location::file(entry.doc_path.clone());
                let summary = format!(
                    "todo_density: {} marker(s) in this doc (TODO/FIXME/XXX/TBD/etc.)",
                    entry.marker_count,
                );
                let seed = format!("todo_density:{}", entry.doc_path.to_string_lossy());
                Finding::new("todo_density", primary, summary, &seed)
            })
            .collect()
    }
}

pub struct TodoDensityFeature;

impl Feature for TodoDensityFeature {
    fn meta(&self) -> FeatureMeta {
        FeatureMeta {
            name: "todo_density",
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
        let Some(report) = reports.todo_density.as_ref() else {
            return Vec::new();
        };
        report
            .into_findings()
            .into_iter()
            .zip(report.entries.iter())
            .map(|(finding, entry)| decorate(finding, classify(entry.marker_count), hotspot))
            .collect()
    }
}
