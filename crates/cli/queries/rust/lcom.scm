; LCOM 近似: impl ブロックを「クラス」相当として捕捉。methods / field-refs
; の抽出は Rust 側 (`observer::lcom`) で AST を walk しながら行う。
;
; trait impl とふつうの impl は同じ扱い。型情報を要する LCOM4 完全
; 実装は v0.5+ の LSP backend で。

(impl_item) @class.scope
