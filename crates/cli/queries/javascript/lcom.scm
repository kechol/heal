; LCOM approximation — captures the class scope only. Method
; extraction and field-reference walking happen in Rust
; (`observer::lcom`) since narrowing "method_definition directly
; under class_body" and "this.X references inside a method" purely
; with tree-sitter queries is awkward.
;
; v0.2 treats abstract / derived classes / mixins uniformly as
; `class_declaration`. A type-aware LCOM4 implementation lands with
; the LSP backend in v0.5+.

(class_declaration) @class.scope
