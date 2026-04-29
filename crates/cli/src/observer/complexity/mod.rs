//! Per-function complexity analysis: parse a source file, extract every
//! function-shaped scope, then compute classical CCN and Sonar-style Cognitive
//! Complexity for each scope.

use std::ops::Range;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser, QueryCursor, StreamingIterator, Tree};

use crate::observer::lang::Language;

mod ccn;
mod cognitive;
mod observer;

pub use observer::{
    ComplexityMetric, ComplexityObserver, ComplexityReport, ComplexityTotals, FileComplexity,
    FunctionFinding,
};

pub(super) const LOGICAL_OPERATORS: &[&str] = &["&&", "||", "??"];

/// A parsed source file ready for repeated metric computations. Holds the
/// owned `source` so node byte ranges remain valid for the tree's lifetime.
pub struct ParsedFile {
    pub source: String,
    pub lang: Language,
    pub tree: Tree,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionScope {
    pub name: String,
    pub byte_range: Range<usize>,
    /// 1-based, inclusive — friendly for editor jump-to-line.
    pub start_row: u32,
    pub end_row: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionMetric {
    pub name: String,
    pub start_line: u32,
    pub end_line: u32,
    pub ccn: u32,
    pub cognitive: u32,
}

/// Returns an error only if the parser itself bails (`set_language` failure or
/// `parse` returning None) — partial parses with ERROR nodes still succeed,
/// since real-world code in editor hooks is often mid-edit.
pub fn parse(source: String, lang: Language) -> Result<ParsedFile> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.ts_language())
        .with_context(|| format!("failed to load {} grammar", lang.name()))?;
    let tree = parser
        .parse(&source, None)
        .ok_or_else(|| anyhow!("tree-sitter returned no tree for {} input", lang.name()))?;
    Ok(ParsedFile { source, lang, tree })
}

#[must_use]
pub fn extract_functions(parsed: &ParsedFile) -> Vec<FunctionScope> {
    collect_scopes(parsed)
        .into_iter()
        .map(|(scope, _)| scope)
        .collect()
}

#[must_use]
pub fn analyze(parsed: &ParsedFile) -> Vec<FunctionMetric> {
    let scope_nodes = collect_scopes(parsed);
    let nested_starts: Vec<usize> = scope_nodes
        .iter()
        .map(|(s, _)| s.byte_range.start)
        .collect();

    scope_nodes
        .into_iter()
        .map(|(scope, node)| FunctionMetric {
            name: scope.name,
            start_line: scope.start_row,
            end_line: scope.end_row,
            ccn: ccn::compute(parsed, node, &nested_starts),
            cognitive: cognitive::compute(parsed, node, &nested_starts),
        })
        .collect()
}

/// Collects function scopes paired with their tree-sitter `Node`s, sorted by
/// start byte. Carrying the node alongside the serializable scope avoids a
/// `descendant_for_byte_range` round-trip in `analyze`.
fn collect_scopes(parsed: &ParsedFile) -> Vec<(FunctionScope, Node<'_>)> {
    let q = parsed.lang.functions_query();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&q.query, parsed.tree.root_node(), parsed.source.as_bytes());

    let mut scopes = Vec::new();
    while let Some(m) = matches.next() {
        for cap in m.captures.iter().filter(|c| c.index == q.captures.scope) {
            scopes.push((scope_from_node(cap.node, &parsed.source), cap.node));
        }
    }
    scopes.sort_by_key(|(s, _)| s.byte_range.start);
    scopes
}

fn scope_from_node(node: Node<'_>, source: &str) -> FunctionScope {
    let name = resolve_function_name(node, source);
    let byte_range = node.start_byte()..node.end_byte();
    let start_row = u32::try_from(node.start_position().row + 1).unwrap_or(u32::MAX);
    let end_row = u32::try_from(node.end_position().row + 1).unwrap_or(u32::MAX);
    FunctionScope {
        name,
        byte_range,
        start_row,
        end_row,
    }
}

/// Best-effort name resolution: prefer the node's own `name` field, then walk
/// the parent chain for binding context (TS `variable_declarator`, JS `pair`,
/// JS `assignment_expression`, Rust `let_declaration`). Falls back to
/// `<anonymous@LINE>`.
fn resolve_function_name(node: Node<'_>, source: &str) -> String {
    if let Some(name_node) = node.child_by_field_name("name") {
        if let Ok(text) = name_node.utf8_text(source.as_bytes()) {
            return text.to_string();
        }
    }

    if let Some(parent) = node.parent() {
        let lhs = match parent.kind() {
            "variable_declarator" | "assignment_expression" => parent
                .child_by_field_name("name")
                .or_else(|| parent.child_by_field_name("left")),
            "pair" => parent.child_by_field_name("key"),
            // Rust: `let f = |x| ...` — pattern field carries the binding.
            // For destructured patterns we get the literal source slice, which
            // is still more useful than `<anonymous>`.
            "let_declaration" => parent.child_by_field_name("pattern"),
            _ => None,
        };
        if let Some(lhs) = lhs {
            if let Ok(text) = lhs.utf8_text(source.as_bytes()) {
                return text.to_string();
            }
        }
    }

    format!("<anonymous@{}>", node.start_position().row + 1)
}

/// Returns true when `node` lies inside a function-shaped scope nested between
/// itself and the scope at `current_start`. Walks ancestors; the first scope
/// hit decides — if it's `current_start`, the node belongs to that scope; if
/// it's any other entry in `nested_starts`, it belongs to the nested function.
///
/// `nested_starts` MUST be sorted ascending (`binary_search`).
pub(crate) fn is_inside_nested_function(
    node: Node<'_>,
    nested_starts: &[usize],
    current_start: usize,
) -> bool {
    let mut cursor = node.parent();
    while let Some(parent) = cursor {
        let start = parent.start_byte();
        if start == current_start {
            return false;
        }
        if nested_starts.binary_search(&start).is_ok() {
            return true;
        }
        cursor = parent.parent();
    }
    false
}
