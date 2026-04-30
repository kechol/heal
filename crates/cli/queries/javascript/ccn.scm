; Classical McCabe Cyclomatic Complexity decision points.
; CCN = 1 + count(captures inside function subtree, excluding nested functions).
;
; Binary expressions are captured generically; the Rust walker filters by
; operator text (&&, ||, ??) since matching anonymous operator nodes via
; query string-literals is grammar-version-fragile.

(if_statement) @ccn.point
(for_statement) @ccn.point
(for_in_statement) @ccn.point
(while_statement) @ccn.point
(do_statement) @ccn.point
(switch_case) @ccn.point
(catch_clause) @ccn.point
(ternary_expression) @ccn.point
(binary_expression) @ccn.binary
