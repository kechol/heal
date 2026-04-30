; LCOM 近似: Python の `class_definition` をクラススコープとして捕捉。
; methods は `function_definition` (suite 直下)、self refs は
; `attribute(object: identifier "self", attribute: ...)` を Rust 側で
; 検出する。`self` は Python ではキーワードではなく単なる識別子なので
; SelfRefShape の receiver 判定はテキスト比較で行う。

(class_definition) @class.scope
