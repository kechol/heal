; Classical McCabe Cyclomatic Complexity decision points for Python.
; CCN = 1 + count(captures inside function subtree, excluding nested functions).
;
; Boolean operators (`and` / `or`) are captured generically; the Rust
; walker counts each occurrence. `match_statement` itself is not a
; decision point — each `case_clause` adds one (mirroring switch_case
; in TS).

(if_statement) @ccn.point
(elif_clause) @ccn.point
(for_statement) @ccn.point
(while_statement) @ccn.point
(except_clause) @ccn.point
(case_clause) @ccn.point
(conditional_expression) @ccn.point
(boolean_operator) @ccn.binary
