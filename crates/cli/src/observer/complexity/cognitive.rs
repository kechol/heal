//! Sonar-style Cognitive Complexity walker.
//!
//! Implements the rules described in
//! <https://www.sonarsource.com/docs/CognitiveComplexity.pdf>:
//!
//! * **B1 (increment)** — each control-flow break adds +1.
//! * **B2 (nesting)** — each break inside a nesting structure adds the
//!   current nesting depth on top of B1.
//! * **B3 (no bonus)** — `else` does not increase nesting; `else if` is a
//!   single +1 (no nesting bonus).
//! * Logical operator chain — `+1` for the chain plus `+1` per operator-kind
//!   switch (`&& → ||`, etc.).
//! * Nested functions — pruned at the cursor; each gets its own row from
//!   `analyze`.

use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, Query, QueryCursor, StreamingIterator};

use super::{ParsedFile, LOGICAL_OPERATORS};
use crate::observer::lang::CognitiveCaptures;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    If,
    Else,
    IncAndNest,
    Inc,
    Binary,
}

pub(crate) fn compute(parsed: &ParsedFile, scope: Node<'_>, nested_starts: &[usize]) -> u32 {
    let q = parsed.lang.cognitive_query();
    let roles = build_role_map(&q.query, &q.captures, scope, parsed.source.as_bytes());

    let mut walker = Walker {
        source: &parsed.source,
        roles,
        visited_binary: HashSet::new(),
        nested_starts,
        scope_start: scope.start_byte(),
        score: 0,
    };
    walker.visit(scope, 0);
    walker.score
}

fn build_role_map(
    query: &Query,
    captures: &CognitiveCaptures,
    scope: Node<'_>,
    source: &[u8],
) -> HashMap<usize, Role> {
    let mut map = HashMap::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, scope, source);
    while let Some(m) = matches.next() {
        for cap in m.captures {
            let role = if cap.index == captures.if_ {
                Role::If
            } else if cap.index == captures.else_ {
                Role::Else
            } else if cap.index == captures.inc_and_nest {
                Role::IncAndNest
            } else if cap.index == captures.inc {
                Role::Inc
            } else if cap.index == captures.binary {
                Role::Binary
            } else {
                continue;
            };
            map.insert(cap.node.id(), role);
        }
    }
    map
}

struct Walker<'p> {
    source: &'p str,
    roles: HashMap<usize, Role>,
    visited_binary: HashSet<usize>,
    nested_starts: &'p [usize],
    scope_start: usize,
    score: u32,
}

impl<'p> Walker<'p> {
    fn visit(&mut self, node: Node<'_>, depth: u32) {
        // Prune nested function bodies — they're scored on their own row.
        // This is the only nested-function check the walker needs; downstream
        // visit_* methods can assume the node belongs to the current scope.
        if node.start_byte() != self.scope_start
            && self.nested_starts.binary_search(&node.start_byte()).is_ok()
        {
            return;
        }

        match self.roles.get(&node.id()).copied() {
            Some(Role::If) => self.visit_if(node, depth),
            Some(Role::Else) => self.visit_else(node, depth),
            Some(Role::IncAndNest) => self.visit_inc_and_nest(node, depth),
            Some(Role::Inc) => self.visit_inc(node, depth),
            Some(Role::Binary) => self.visit_binary(node, depth),
            None => self.visit_children(node, depth),
        }
    }

    fn visit_children(&mut self, node: Node<'_>, depth: u32) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit(child, depth);
        }
    }

    fn visit_if(&mut self, node: Node<'_>, depth: u32) {
        let is_else_if = node.parent().is_some_and(|p| p.kind() == "else_clause");
        if is_else_if {
            // else-if: +1, no nesting bonus, no nesting increase for body.
            self.score = self.score.saturating_add(1);
            self.visit_children(node, depth);
        } else {
            self.score = self.score.saturating_add(1 + depth);
            self.visit_children(node, depth.saturating_add(1));
        }
    }

    fn visit_else(&mut self, node: Node<'_>, depth: u32) {
        // If the else_clause directly wraps an if (TS: if_statement, Rust:
        // if_expression), the inner if — special-cased as else-if in visit_if —
        // absorbs the +1.
        let wraps_if = direct_child_of_kind(node, "if_statement").is_some()
            || direct_child_of_kind(node, "if_expression").is_some();
        if !wraps_if {
            self.score = self.score.saturating_add(1);
        }
        self.visit_children(node, depth);
    }

    fn visit_inc_and_nest(&mut self, node: Node<'_>, depth: u32) {
        self.score = self.score.saturating_add(1 + depth);
        self.visit_children(node, depth.saturating_add(1));
    }

    fn visit_inc(&mut self, node: Node<'_>, depth: u32) {
        // ternary: +1 + depth, but doesn't increase nesting for its branches.
        self.score = self.score.saturating_add(1 + depth);
        self.visit_children(node, depth);
    }

    fn visit_binary(&mut self, node: Node<'_>, depth: u32) {
        if !self.visited_binary.contains(&node.id()) {
            let mut ops: Vec<&'p str> = Vec::new();
            collect_chain_ops(node, &mut ops, &mut self.visited_binary, self.source);
            if !ops.is_empty() {
                let switches = ops.windows(2).filter(|w| w[0] != w[1]).count();
                let switch_count = u32::try_from(switches).unwrap_or(u32::MAX);
                self.score = self.score.saturating_add(1).saturating_add(switch_count);
            }
        }
        self.visit_children(node, depth);
    }
}

fn direct_child_of_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let count = u32::try_from(node.child_count()).unwrap_or(u32::MAX);
    (0..count)
        .filter_map(|i| node.child(i))
        .find(|child| child.kind() == kind)
}

/// In-order traversal of a `binary_expression` chain. Non-logical operators
/// (e.g. `+`, `===`) terminate the chain — their subtrees aren't descended,
/// matching Sonar's "sequence of like operators" definition.
fn collect_chain_ops<'a>(
    node: Node<'_>,
    ops: &mut Vec<&'a str>,
    visited: &mut HashSet<usize>,
    source: &'a str,
) {
    if node.kind() != "binary_expression" {
        return;
    }
    let Some(op_node) = node.child_by_field_name("operator") else {
        return;
    };
    let Ok(op_text) = op_node.utf8_text(source.as_bytes()) else {
        return;
    };
    if !LOGICAL_OPERATORS.contains(&op_text) {
        return;
    }

    visited.insert(node.id());

    if let Some(left) = node.child_by_field_name("left") {
        collect_chain_ops(left, ops, visited, source);
    }
    ops.push(op_text);
    if let Some(right) = node.child_by_field_name("right") {
        collect_chain_ops(right, ops, visited, source);
    }
}
