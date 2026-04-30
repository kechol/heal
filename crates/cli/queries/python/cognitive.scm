; Sonar-style Cognitive Complexity for Python, captured by role.
;
;   @if           — `if_statement`; the walker treats `elif` chains
;                   like else-if (no nesting bonus).
;   @else         — `else_clause`; +1 unless it's an else-if.
;   @inc_and_nest — `for_statement` / `while_statement` / `try_statement`
;                   / `match_statement`; +1 plus current_nesting, raises nesting.
;   @inc          — `conditional_expression` (`x if cond else y`); +1 only.
;   @binary       — `boolean_operator` (`and` / `or`); chain handling
;                   in the walker.

(if_statement) @if
(elif_clause) @if
(else_clause) @else
(for_statement) @inc_and_nest
(while_statement) @inc_and_nest
(try_statement) @inc_and_nest
(match_statement) @inc_and_nest
(conditional_expression) @inc
(boolean_operator) @binary
