; Function-shaped scopes for Go.
; - `function_declaration`: top-level `func Foo(...) { ... }`.
; - `method_declaration`: methods with a receiver
;   (`func (s *Struct) Foo(...) { ... }`).
; - `func_literal`: anonymous funcs / closures.

(function_declaration) @function.scope
(method_declaration) @function.scope
(func_literal) @function.scope
