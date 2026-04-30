; LCOM v0.2 doesn't apply to Go. Go has no class scope; methods attach
; to types via receivers (`func (s *T) Foo() { ... }`) and live at
; module scope alongside free functions. Grouping methods by receiver
; type plus tracking per-method receiver names is a different model
; than the class-aware backend used by TS / JS / Python / Rust.
;
; The capture below is a placeholder so the registry's "every language
; has a non-empty lcom query" invariant holds. The observer's
; `is_method_kind(_, Language::Go) = false`, so `analyze_class` finds
; no methods inside `source_file` and emits no Findings.
;
; A receiver-grouped LCOM lands in v0.3+ alongside the LSP-backed
; backend.

(source_file) @class.scope
