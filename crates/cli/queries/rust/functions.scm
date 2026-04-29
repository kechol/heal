; Function-shaped scopes. `function_item` covers free fns, methods inside
; `impl` blocks, and trait method definitions (with body). Closures get their
; own scope; their name is resolved via the parent `let_declaration`'s pattern
; in resolve_function_name (Rust uses `pattern`, not `name`, on let).

(function_item) @function.scope
(closure_expression) @function.scope
