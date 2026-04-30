; Classical McCabe Cyclomatic Complexity decision points for Go.
; Go has no ternary and no try/catch — control flow goes through if/for/
; switch/select. Each `expression_case` and `type_case` adds one
; (mirroring switch_case in TS).

(if_statement) @ccn.point
(for_statement) @ccn.point
(expression_case) @ccn.point
(type_case) @ccn.point
(communication_case) @ccn.point
(binary_expression) @ccn.binary
