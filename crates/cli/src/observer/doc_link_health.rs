//! Internal-link health: relative paths and intra-page anchors that
//! resolve, vs. relative paths that don't and anchors that point at
//! headings the doc doesn't define.
//!
//! External (`http://`, `https://`, `mailto:`) links are deliberately
//! out of scope — `scope.md` R5 forbids network access, and external
//! link rot is best handled by CI (lychee, linkchecker) anyway.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::config::Config;
use crate::core::doc_pairs::DocPairsFile;
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::core::severity::Severity;
use crate::feature::{decorate, Feature, FeatureKind, FeatureMeta, HotspotIndex};
use crate::observer::doc_markdown::{
    extract_links, is_external, iter_prose_lines, resolve_relative, split_link_target,
};

/// Owns its inputs so the lifecycle matches the rest of the observer
/// family. The caller pre-walks Layer B docs and pre-extracts Layer A
/// doc paths so the observer doesn't need a `&Config`.
pub struct DocLinkHealthObserver {
    enabled: bool,
    standalone_docs: Vec<PathBuf>,
    paired_docs: Vec<PathBuf>,
}

impl DocLinkHealthObserver {
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

    /// Scan every supplied doc body for relative links / anchors and
    /// emit one finding per broken link.
    #[must_use]
    pub fn scan(&self, root: &Path) -> DocLinkHealthReport {
        let mut report = DocLinkHealthReport::default();
        if !self.enabled {
            return report;
        }
        let mut targets: Vec<PathBuf> = self.standalone_docs.clone();
        for paired in &self.paired_docs {
            if !targets.contains(paired) {
                targets.push(paired.clone());
            }
        }
        let mut total_links = 0usize;
        for rel in &targets {
            let abs = root.join(rel);
            let Ok(body) = std::fs::read_to_string(&abs) else {
                continue;
            };
            let headings = collect_heading_ids(&body);
            for link in extract_links(&body) {
                if is_external(&link.target) {
                    continue;
                }
                total_links += 1;
                let (path_part, anchor_part) = split_link_target(&link.target);
                let resolved = if path_part.is_empty() {
                    rel.clone()
                } else {
                    resolve_relative(rel, path_part)
                };
                let abs_resolved = root.join(&resolved);
                let path_ok = abs_resolved.exists();
                if !path_ok {
                    report.entries.push(DocLinkHealthEntry {
                        doc_path: rel.clone(),
                        line: link.line,
                        target: link.target.clone(),
                        kind: LinkBreakKind::MissingPath,
                    });
                    continue;
                }
                if !anchor_part.is_empty()
                    && path_part.is_empty()
                    && !headings.contains(anchor_part)
                {
                    // Same-doc anchor — verify against this doc's own headings.
                    report.entries.push(DocLinkHealthEntry {
                        doc_path: rel.clone(),
                        line: link.line,
                        target: link.target.clone(),
                        kind: LinkBreakKind::MissingAnchor,
                    });
                }
            }
        }
        report.entries.sort_by(|a, b| {
            a.doc_path
                .cmp(&b.doc_path)
                .then_with(|| a.line.cmp(&b.line))
                .then_with(|| a.target.cmp(&b.target))
        });
        report.totals = DocLinkHealthTotals {
            scanned_docs: targets.len(),
            scanned_links: total_links,
            broken: report.entries.len(),
        };
        report
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocLinkHealthReport {
    pub entries: Vec<DocLinkHealthEntry>,
    pub totals: DocLinkHealthTotals,
}

impl DocLinkHealthReport {
    #[must_use]
    pub fn worst_n(&self, n: usize) -> Vec<DocLinkHealthEntry> {
        let mut top = self.entries.clone();
        top.truncate(n);
        top
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocLinkHealthEntry {
    pub doc_path: PathBuf,
    pub line: u32,
    pub target: String,
    pub kind: LinkBreakKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkBreakKind {
    MissingPath,
    MissingAnchor,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocLinkHealthTotals {
    pub scanned_docs: usize,
    pub scanned_links: usize,
    pub broken: usize,
}

/// Collect every heading id defined in the doc. Markdown's GitHub-
/// flavored heading id rule is: lower-case, replace spaces with `-`,
/// drop punctuation. We approximate by lower-casing and replacing
/// whitespace runs with `-`.
fn collect_heading_ids(body: &str) -> HashSet<String> {
    iter_prose_lines(body)
        .filter_map(|(_, line)| {
            let trimmed = line.trim_start();
            let after_hash = trimmed.strip_prefix('#')?;
            let body = after_hash.trim_start_matches('#').trim_start();
            Some(slugify(body))
        })
        .collect()
}

fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if (ch.is_whitespace() || matches!(ch, '-' | '_')) && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

impl IntoFindings for DocLinkHealthReport {
    fn into_findings(&self) -> Vec<Finding> {
        self.entries
            .iter()
            .map(|entry| {
                let primary = Location {
                    file: entry.doc_path.clone(),
                    line: Some(entry.line),
                    symbol: None,
                };
                let summary = match entry.kind {
                    LinkBreakKind::MissingPath => format!(
                        "doc_link_health: relative link target `{}` does not exist",
                        entry.target,
                    ),
                    LinkBreakKind::MissingAnchor => format!(
                        "doc_link_health: anchor `{}` not defined in this doc",
                        entry.target,
                    ),
                };
                let seed = format!(
                    "doc_link_health:{}:{}",
                    entry.doc_path.to_string_lossy(),
                    entry.target,
                );
                Finding::new("doc_link_health", primary, summary, &seed)
            })
            .collect()
    }
}

/// Extract the doc paths from a [`DocPairsFile`] for the link-health
/// observer's combined Layer A + Layer B sweep.
#[must_use]
pub fn paired_doc_paths(file: Option<&DocPairsFile>) -> Vec<PathBuf> {
    file.map(|f| f.pairs.iter().map(|p| PathBuf::from(&p.doc)).collect())
        .unwrap_or_default()
}

pub struct DocLinkHealthFeature;

impl Feature for DocLinkHealthFeature {
    fn meta(&self) -> FeatureMeta {
        FeatureMeta {
            name: "doc_link_health",
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
        let Some(report) = reports.doc_link_health.as_ref() else {
            return Vec::new();
        };
        // Internal link breaks are mechanical to fix and high-impact —
        // a reader who follows a broken link reaches a 404. High floors
        // accordingly; per-team softer floors go through
        // `[policy.drain.metrics.doc_link_health]`.
        report
            .into_findings()
            .into_iter()
            .map(|f| decorate(f, Severity::High, hotspot))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_matches_github_style() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("API Reference"), "api-reference");
        assert_eq!(slugify("Multi  spaces"), "multi-spaces");
    }
}
