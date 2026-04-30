; Sonar-style Cognitive Complexity for Go, captured by role.
;
;   @if           — `if_statement`; the walker treats `else if` chains
;                   like else-if (no nesting bonus).
;   @else         — the `else` block of an `if_statement` (alternative
;                   is a `block`, not another `if_statement`); +1 once
;                   per final else.
;   @inc_and_nest — `for_statement` /
;                   `expression_switch_statement` / `type_switch_statement` /
;                   `select_statement`; +1 plus current_nesting, raises nesting.
;   @inc          — `goto_statement`; non-linear flow gets +1 per Sonar.
;                   Go has no ternary expression, so this is the only
;                   structural increment beyond loops / branches.
;   @binary       — `binary_expression` (`&&` / `||`); chain handling
;                   in the walker.

(if_statement) @if
(if_statement alternative: (block) @else)
(for_statement) @inc_and_nest
(expression_switch_statement) @inc_and_nest
(type_switch_statement) @inc_and_nest
(select_statement) @inc_and_nest
(goto_statement) @inc
(binary_expression) @binary
