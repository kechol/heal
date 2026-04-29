; McCabe Cyclomatic Complexity decision points for Rust.
;
; `try_expression` (the `?` operator) is included as a +1 — it desugars to an
; early-return Err branch, which is structurally a decision point. Sonar's PDF
; doesn't formalize this; the choice is documented in the cognitive query too.
;
; `binary_expression` is captured generically; the Rust walker filters by
; operator text (`&&`, `||`) — Rust has no `??`.

(if_expression) @ccn.point
(while_expression) @ccn.point
(for_expression) @ccn.point
(loop_expression) @ccn.point
(match_arm) @ccn.point
(try_expression) @ccn.point
(binary_expression) @ccn.binary
