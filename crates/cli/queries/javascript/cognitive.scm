; Sonar-style Cognitive Complexity, captured by role for the walker.
;
;   @if           — handled specially: else-if branches add +1 only (no nesting bonus).
;   @else         — else clause; +1 unless it's an else-if (the inner if absorbs the +1).
;   @inc_and_nest — adds (1 + current_nesting) and increases nesting for its body.
;   @inc          — adds (1 + current_nesting), does NOT increase nesting (e.g. ternary).
;   @binary       — logical operator chain entry; walker counts +1 per chain plus
;                   +1 per operator-kind switch within the chain (Sonar PDF §1.2).
;
; Nested functions are handled by the walker (they get their own row and reset
; nesting), so we don't try to encode that in the query.

(if_statement) @if
(else_clause) @else
(for_statement) @inc_and_nest
(for_in_statement) @inc_and_nest
(while_statement) @inc_and_nest
(do_statement) @inc_and_nest
(switch_statement) @inc_and_nest
(catch_clause) @inc_and_nest
(ternary_expression) @inc
(binary_expression) @binary
