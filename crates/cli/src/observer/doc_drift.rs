//! Doc ⇆ src identifier drift (Type 1: dangling identifier).
//!
//! For every doc ⇔ src(s) entry, scan the doc body for inline
//! `` `identifier` `` spans, parse each src with tree-sitter, and surface
//! any identifier the doc mentions that no longer exists in the
//! corresponding source AST. Type 2 drift (signature mismatch) and
//! Type 3 (semantic drift) need per-language code-block parsing or LLM
//! reasoning respectively and are deliberately deferred to v0.5+.
//!
//! ## Why dangling identifiers are Critical
//!
//! A reader who jumps to a doc and types a name that no longer exists
//! ends up at a worse outcome than no doc at all — the doc actively
//! misdirects them. The observer-side severity stays `Ok`; the Feature
//! pass classifies every emitted finding as `Severity::Critical`. Users
//! can override via `[policy.drain.metrics.doc_drift]` if their
//! particular convention demands a softer floor.
//!
//! ## What does *not* count as a dangling identifier
//!
//! 1. Tokens inside fenced code blocks (` ``` … ``` `, ` ~~~ … ~~~ `).
//!    These are usage examples, often illustrating obsolete shapes
//!    intentionally for migration guides.
//! 2. Tokens that aren't identifier-shaped: bare punctuation, numbers,
//!    URL fragments, single keywords like `if` or `let` — the scanner
//!    requires at least one alphabetic character and an identifier-ish
//!    overall shape.
//! 3. Tokens that match anywhere in any src AST in the pair, even as a
//!    field, type, macro, lifetime — exact-string match against the
//!    flat set of leaf tokens.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use tree_sitter::TreeCursor;

use crate::core::config::Config;
use crate::core::doc_pairs::DocPair;
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::core::severity::Severity;
use crate::feature::{decorate, Feature, FeatureKind, FeatureMeta, HotspotIndex};
use crate::observer::complexity::parse;
use crate::observer::lang::Language;

#[derive(Debug, Clone, Default)]
pub struct DocDriftObserver {
    pub enabled: bool,
    pub pairs: Vec<DocPair>,
}

impl DocDriftObserver {
    #[must_use]
    pub fn from_config_and_pairs(cfg: &Config, pairs: Vec<DocPair>) -> Self {
        Self {
            enabled: cfg.features.docs.enabled,
            pairs,
        }
    }

    /// Read each doc + paired srcs from disk, extract doc-side
    /// `` `identifier` `` spans, and emit one entry per dangling
    /// identifier.
    #[must_use]
    pub fn scan(&self, root: &Path) -> DocDriftReport {
        let mut report = DocDriftReport::default();
        if !self.enabled || self.pairs.is_empty() {
            return report;
        }
        // Memoise per-src identifier sets — tree-sitter parsing is
        // expensive and the same src commonly appears in several pairs
        // (e.g. one src documented from a CLI page and an architecture
        // page). `Rc` so multiple pairs can share a set without
        // cloning the contents.
        let mut src_cache: HashMap<PathBuf, Rc<HashSet<String>>> = HashMap::new();
        for pair in &self.pairs {
            let doc_path = root.join(&pair.doc);
            let Ok(doc_text) = std::fs::read_to_string(&doc_path) else {
                continue;
            };
            let mentions = extract_inline_identifiers(&doc_text);
            if mentions.is_empty() {
                continue;
            }
            let mut combined: HashSet<String> = HashSet::new();
            for src in &pair.srcs {
                let key = PathBuf::from(src);
                let set = src_cache
                    .entry(key.clone())
                    .or_insert_with(|| Rc::new(parse_src_identifiers(&root.join(src))));
                combined.extend(set.iter().cloned());
            }
            for mention in mentions {
                if combined.contains(&mention.text) {
                    continue;
                }
                report.entries.push(DocDriftEntry {
                    doc_path: PathBuf::from(&pair.doc),
                    src_paths: pair.srcs.iter().map(PathBuf::from).collect(),
                    identifier: mention.text,
                    doc_line: mention.line,
                });
            }
        }
        report.totals = DocDriftTotals {
            dangling_identifiers: report.entries.len(),
        };
        // Stable order: by (doc, line, identifier).
        report.entries.sort_by(|a, b| {
            a.doc_path
                .cmp(&b.doc_path)
                .then_with(|| a.doc_line.cmp(&b.doc_line))
                .then_with(|| a.identifier.cmp(&b.identifier))
        });
        report
    }
}

/// Read `src_path`, parse with the matching tree-sitter grammar, and
/// collect every identifier-shaped leaf token. Returns an empty set
/// when the file is unsupported, missing, or fails to parse — caller
/// checks the doc's own mention list against the union of these sets.
fn parse_src_identifiers(src_path: &Path) -> HashSet<String> {
    let mut out = HashSet::new();
    let Some(lang) = Language::from_path(src_path) else {
        return out;
    };
    let Ok(src_text) = std::fs::read_to_string(src_path) else {
        return out;
    };
    let Ok(parsed) = parse(src_text, lang) else {
        return out;
    };
    collect_identifier_tokens(&parsed.tree, parsed.source.as_bytes(), &mut out);
    out
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocDriftReport {
    pub entries: Vec<DocDriftEntry>,
    pub totals: DocDriftTotals,
}

impl DocDriftReport {
    #[must_use]
    pub fn worst_n(&self, n: usize) -> Vec<DocDriftEntry> {
        let mut top = self.entries.clone();
        top.truncate(n);
        top
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocDriftEntry {
    pub doc_path: PathBuf,
    pub src_paths: Vec<PathBuf>,
    pub identifier: String,
    pub doc_line: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocDriftTotals {
    pub dangling_identifiers: usize,
}

#[derive(Debug, Clone)]
struct InlineMention {
    text: String,
    line: u32,
}

/// Walk a Markdown body and pull out every inline-code identifier (text
/// inside a single pair of backticks) outside of fenced code blocks.
/// Identifier shape: at least one ASCII alphabetic character, otherwise
/// only `[A-Za-z0-9_:.<>-]` with an alpha somewhere in the body. Pure
/// punctuation and numeric strings drop out, as does anything containing
/// whitespace or backticks.
fn extract_inline_identifiers(text: &str) -> Vec<InlineMention> {
    let mut out = Vec::new();
    for (line_no, line) in crate::observer::doc_markdown::iter_prose_lines(text) {
        scan_line_for_inline(line, line_no, &mut out);
    }
    out
}

fn scan_line_for_inline(line: &str, line_no: u32, out: &mut Vec<InlineMention>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        // Skip double-backtick spans entirely — they're often used to
        // embed example code with backticks inside, where identifier
        // extraction is ambiguous.
        if i + 1 < bytes.len() && bytes[i + 1] == b'`' {
            i += 2;
            // Find closing ``
            while i + 1 < bytes.len() && !(bytes[i] == b'`' && bytes[i + 1] == b'`') {
                i += 1;
            }
            i = i.saturating_add(2);
            continue;
        }
        // Single-backtick span.
        let start = i + 1;
        let mut end = start;
        while end < bytes.len() && bytes[end] != b'`' {
            end += 1;
        }
        if end >= bytes.len() {
            // Unclosed span — bail on the rest of the line.
            return;
        }
        let span = &line[start..end];
        if is_identifier_shape(span) {
            out.push(InlineMention {
                text: span.to_owned(),
                line: line_no,
            });
        }
        i = end + 1;
    }
}

/// Token shape acceptable as an identifier mention. Must contain at
/// least one ASCII alphabetic character, and otherwise consist only of
/// identifier-ish characters (alphanumerics + the limited punctuation
/// users routinely embed when referencing items: `_`, `:`, `.`, `<`,
/// `>`, `-`). Whitespace, backticks, single-quotes, parens, and brackets
/// disqualify the span — those usually indicate prose rather than a
/// symbol reference.
fn is_identifier_shape(span: &str) -> bool {
    if span.is_empty() {
        return false;
    }
    let mut has_alpha = false;
    for ch in span.chars() {
        if ch.is_ascii_alphabetic() {
            has_alpha = true;
        }
        if !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':' | '.' | '<' | '>' | '-')) {
            return false;
        }
    }
    has_alpha
}

/// Walk the tree-sitter tree and collect every leaf node whose text
/// looks like an identifier. We don't filter by node kind because
/// kind names vary across grammars — alphanumeric leaves are a robust
/// approximation that matches what `extract_inline_identifiers` emits.
fn collect_identifier_tokens(tree: &tree_sitter::Tree, source: &[u8], out: &mut HashSet<String>) {
    let mut cursor: TreeCursor<'_> = tree.walk();
    loop {
        let node = cursor.node();
        if node.child_count() == 0 && !node.is_extra() && !node.is_error() {
            if let Ok(text) = node.utf8_text(source) {
                let trimmed = text.trim();
                if is_identifier_shape(trimmed) {
                    out.insert(trimmed.to_owned());
                }
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

impl IntoFindings for DocDriftReport {
    fn into_findings(&self) -> Vec<Finding> {
        self.entries
            .iter()
            .map(|entry| {
                let primary = Location {
                    file: entry.doc_path.clone(),
                    line: Some(entry.doc_line),
                    symbol: None,
                };
                let locations: Vec<Location> = entry
                    .src_paths
                    .iter()
                    .map(|p| Location::file(p.clone()))
                    .collect();
                let summary = format!(
                    "doc_drift: doc references `{}` but no paired src defines it",
                    entry.identifier,
                );
                let seed = format!(
                    "doc_drift:{}:{}",
                    entry.doc_path.to_string_lossy(),
                    entry.identifier,
                );
                Finding::new("doc_drift", primary, summary, &seed).with_locations(locations)
            })
            .collect()
    }
}

pub struct DocDriftFeature;

impl Feature for DocDriftFeature {
    fn meta(&self) -> FeatureMeta {
        FeatureMeta {
            name: "doc_drift",
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
        let Some(report) = reports.doc_drift.as_ref() else {
            return Vec::new();
        };
        report
            .into_findings()
            .into_iter()
            .map(|f| decorate(f, Severity::Critical, hotspot))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_inline_identifiers_outside_fences() {
        let body = "Use `Foo::bar` to do X.\n\n```rust\nlet `Baz` = 1;\n```\n\nSee `Qux`.";
        let mentions = extract_inline_identifiers(body);
        let names: Vec<String> = mentions.iter().map(|m| m.text.clone()).collect();
        assert!(names.contains(&"Foo::bar".to_string()));
        assert!(names.contains(&"Qux".to_string()));
        assert!(!names.contains(&"Baz".to_string()), "fenced span leaked");
    }

    #[test]
    fn skips_double_backtick_spans() {
        let body = "Embed ``with `nested` backticks`` here, plus `Real`.";
        let mentions = extract_inline_identifiers(body);
        let names: Vec<String> = mentions.iter().map(|m| m.text.clone()).collect();
        assert_eq!(names, vec!["Real".to_string()]);
    }

    #[test]
    fn ignores_non_identifier_shape() {
        let body = "Numbers `123`, prose `hello world`, punct `()`. But `Real_id`.";
        let mentions = extract_inline_identifiers(body);
        let names: Vec<String> = mentions.iter().map(|m| m.text.clone()).collect();
        assert_eq!(names, vec!["Real_id".to_string()]);
    }
}
