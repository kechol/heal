; Function-shaped scopes for per-function complexity attribution.
; Capture only the scope node — names are resolved in Rust by inspecting
; child_by_field_name("name") and walking the parent chain (variable_declarator,
; pair, assignment_expression) so anonymous arrows still get a useful label.

(function_declaration) @function.scope
(generator_function_declaration) @function.scope
(method_definition) @function.scope
(function_expression) @function.scope
(arrow_function) @function.scope
