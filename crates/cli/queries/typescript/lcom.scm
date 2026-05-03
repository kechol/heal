; LCOM approximation: capture only the class scope here. Methods and
; field-refs are extracted on the Rust side (`observer::lcom`) by
; walking the AST — narrowing to "method_definition directly under
; class_body" and "this.X references inside a method" with tree-sitter
; queries alone is unwieldy.
;
; abstract / derived classes / mixins are all handled uniformly as
; `class_declaration` for now. A full LCOM4 implementation that needs
; type information is reserved for the v0.5+ LSP backend.

(class_declaration) @class.scope
