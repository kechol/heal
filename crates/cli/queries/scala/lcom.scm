; LCOM v0.2 doesn't apply to Scala. Scala's class story spans
; class / trait / object / case-class / given (Scala 3) constructs,
; and the `this.field` access pattern is rarer than in TS / Python
; (most member accesses are bare names resolved by scope). A
; class-aware LCOM that handles this richness needs the LSP backend,
; which lands in v0.5+.
;
; The capture below is a placeholder so the registry's "every language
; has a non-empty lcom query" invariant holds. The observer's
; `is_method_kind(_, Language::Scala) = false`, so `analyze_class` finds
; no methods and emits no Findings.

(compilation_unit) @class.scope
