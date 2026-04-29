; Sonar-style Cognitive Complexity captures for Rust.
;
; Structurally identical to the TypeScript query: tree-sitter-rust uses the
; same `else_clause` node shape (containing either a `block` or another
; `if_expression`), so the walker's existing else-if collapse logic works
; without language-specific dispatch.
;
;   @if            — handled with the else-if special-case (parent == else_clause)
;   @else          — else_clause; +1 unless it directly wraps an if_expression
;   @inc_and_nest  — adds (1 + current_nesting) and bumps nesting for body
;   @inc           — flat +1 (no nesting bonus); used for `?` like ternary in TS
;   @binary        — logical operator chain entry
;
; `match_arm` is intentionally NOT captured: Sonar treats the match as one
; increment with arms inheriting the bumped nesting (consistent with switch).

(if_expression) @if
(else_clause) @else
(while_expression) @inc_and_nest
(for_expression) @inc_and_nest
(loop_expression) @inc_and_nest
(match_expression) @inc_and_nest
(try_expression) @inc
(binary_expression) @binary
