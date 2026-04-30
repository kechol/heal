; Sonar-style Cognitive Complexity for Scala, captured by role.
;
;   @if           — `if_expression`; the walker treats `else if` chains
;                   like else-if (no nesting bonus).
;   @else         — `else` branch when it's a plain block (not an
;                   `if_expression`); +1 once.
;   @inc_and_nest — `for_expression` / `while_expression` / `do_expression`
;                   / `match_expression` / `try_expression`; +1 plus
;                   current_nesting, raises nesting.
;   @inc          — placeholder; Scala's `throw` is treated as a +1
;                   non-linear flow per Sonar.
;   @binary       — `infix_expression` (the walker filters by operator
;                   text — `&&` / `||`).

(if_expression) @if
(if_expression alternative: (block) @else)
(for_expression) @inc_and_nest
(while_expression) @inc_and_nest
(match_expression) @inc_and_nest
(try_expression) @inc_and_nest
(throw_expression) @inc
(infix_expression) @binary
