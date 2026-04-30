; Function-shaped scopes. `function_definition` covers top-level `def`
; statements, methods inside a class body, and nested defs. Lambdas
; get their own scope; their name is resolved via the parent
; assignment / keyword argument in resolve_function_name.

(function_definition) @function.scope
(lambda) @function.scope
