; LCOM approximation: capture Python `class_definition` as the class
; scope. Methods (`function_definition` directly under the class suite)
; and self-references (`attribute(object: identifier "self", attribute:
; …)`) are extracted on the Rust side. `self` is an ordinary identifier
; in Python rather than a keyword, so the receiver check in
; `SelfRefShape` falls back to a text comparison.

(class_definition) @class.scope
