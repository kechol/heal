//! Classical `McCabe` Cyclomatic Complexity per function.
//!
//! `1 (baseline) + count(decision-point captures inside the scope, excluding
//! captures that fall inside a nested function)`.

use tree_sitter::{Node, QueryCursor, StreamingIterator};

use super::{is_inside_nested_function, ParsedFile, LOGICAL_OPERATORS};

pub(crate) fn compute(parsed: &ParsedFile, scope: Node<'_>, nested_starts: &[usize]) -> u32 {
    let q = parsed.lang.ccn_query();
    let scope_start = scope.start_byte();
    let mut count: u32 = 0;

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&q.query, scope, parsed.source.as_bytes());
    while let Some(m) = matches.next() {
        for cap in m.captures {
            if is_inside_nested_function(cap.node, nested_starts, scope_start) {
                continue;
            }
            let counts = cap.index == q.captures.point
                || (cap.index == q.captures.binary
                    && is_logical_operator(cap.node, &parsed.source));
            if counts {
                count = count.saturating_add(1);
            }
        }
    }

    count.saturating_add(1)
}

fn is_logical_operator(node: Node<'_>, source: &str) -> bool {
    let Some(op) = node.child_by_field_name("operator") else {
        return false;
    };
    let Ok(text) = op.utf8_text(source.as_bytes()) else {
        return false;
    };
    LOGICAL_OPERATORS.contains(&text)
}
