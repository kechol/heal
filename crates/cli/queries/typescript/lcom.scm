; LCOM 近似: クラススコープのみ捕捉。methods / field-refs の抽出は
; Rust 側 (`observer::lcom`) で AST を walk しながら行う — tree-sitter
; query だけで「class_body 直下の method_definition」「method 内の
; this.X 参照」までを正確に絞り込むのが煩雑なため。
;
; abstract / 派生クラス / mixin は v0.2 では一律 class_declaration として
; 扱う。型情報を要する LCOM4 完全実装は v0.5+ の LSP backend で。

(class_declaration) @class.scope
