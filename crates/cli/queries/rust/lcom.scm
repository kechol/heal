; LCOM approximation: capture `impl` blocks as the "class" equivalent.
; Methods and field-refs are extracted on the Rust side
; (`observer::lcom`) by walking the AST.
;
; Trait impls and inherent impls are treated the same. A full LCOM4
; implementation that needs type information is reserved for the
; v0.5+ LSP backend.

(impl_item) @class.scope
