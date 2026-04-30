; Function-shaped scopes for Scala.
; - `function_definition`: `def foo(...) = body`.
; - `function_declaration`: abstract def signature in a trait.
; - `lambda_expression`: anonymous functions / closures.

(function_definition) @function.scope
(function_declaration) @function.scope
(lambda_expression) @function.scope
