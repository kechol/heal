---
title: Docs · 設定
description: "[features.docs] の有効化、standalone ドキュメントの選択、freshness フロアの調整、scaffold ルートの設定、.heal/doc_pairs.json の理解。"
---

**Docs** ファミリはオプトインです。デフォルトでオフ。コードメトリクスと並べて古いドキュメントを表面化したくなったら有効化してください。外部 HTTP リンクのチェックやサンプルコードの実行はスコープ外です(heal はローカル限定で動きます。HTTP 側は CI で `lychee` などを使ってください)。

各メトリクスが捕まえる内容は [Docs › メトリクス](/heal/ja/docs/metrics/)、同梱スキルは [Docs › スキル](/heal/ja/docs/skills/) を参照。

## サクッと有効化

```toml
[features.docs]
enabled = true
```

その後 `/heal-doc-pair-setup` を 1 回実行して `.heal/doc_pairs.json` を populate します。heal はこのファイルの読み取り専用消費者です(下記の [`.heal/doc_pairs.json`](#heal-doc_pairsjson--ペアファイル) を参照)。

## `[features.docs]`

```toml
[features.docs]
enabled       = false                        # マスタースイッチ
pairs_path    = ".heal/doc_pairs.json"       # ペアファイルの位置
scaffold_root = ".heal/docs"                 # /heal-doc-scaffold の出力先
```

- `enabled`(デフォルト `false`) — マスタースイッチ。false の間は全 docs オブザーバが no-op になり、`.heal/doc_pairs.json` も参照されません。
- `pairs_path`(デフォルト `.heal/doc_pairs.json`) — ペアファイルへのプロジェクト相対パス。heal は読むだけで、生成は `/heal-doc-pair-setup` の役割です。
- `scaffold_root`(デフォルト `.heal/docs`) — `/heal-doc-scaffold` が Markdown skeleton を書き出すプロジェクト相対のルート。heal 本体はこのツリーを読み書きしません — チームメンバーが scaffold を再生成しても同じ場所に揃うようにするための消費者向けメタデータです。デフォルトを `.heal/docs` にしているのは、プロジェクトが既に持つ `docs/`(Starlight / mdBook / mkdocs)と衝突させないため。skeleton を確認したら `git mv .heal/docs docs` で公開ロケーションに昇格させ、`scaffold_root = "docs"` に書き換えると次回以降は直接そこに生成されます。

## `[features.docs.standalone]`

```toml
[features.docs.standalone]
include = ["**/*.md", "**/*.rst"]
exclude = [
  "CHANGELOG*", "CHANGELOG/**",
  "CONTRIBUTING*",
  "CODE_OF_CONDUCT*",
  "SECURITY*",
  "**/adr/**",
  "target/**", "dist/**", "node_modules/**",
]
```

`standalone` は **Layer B** ドキュメント — リンク / 孤立 / TODO のチェックは必要だがペアマッチングは不要な prose ドキュメント(README、コンセプトガイド、説明ページ)です。

デフォルトの `exclude` リストが除外しているもの:

- ガバナンス / 履歴ファイル(`CHANGELOG*`、`CONTRIBUTING*`、`CODE_OF_CONDUCT*`、`SECURITY*`) — 日付付きの履歴にドリフト検出は適用されません。
- ADR(`**/adr/**`) — 慣例として merge 後は編集されないため。
- 生成された API リファレンスとビルド成果物。

デフォルトでカバーされない生成 docs(例: `docs/api-generated/` ツリー)があるときは `exclude` に追加します。

## `[features.docs.doc_freshness]`

```toml
[features.docs.doc_freshness]
high_commits     = 5    # ドキュメントを過ぎたソースコミット数 → High severity
critical_commits = 20   # ドキュメントを過ぎたソースコミット数 → Critical severity
```

絶対 commit-distance フロアです。距離は日数ではなくコミット数で測るので、チームのコミットペースが変わってもしきい値はずれません。

適用ルール:

| `src_commits_since_doc ≥` | Severity |
|---|---|
| `critical_commits` | Critical |
| `high_commits` | High |
| 1 | Medium |

両フロアを下げると締まり、上げると緩みます。

## `[features.docs.todo_density]`

```toml
[features.docs.todo_density]
ignore_in_inline_code = true   # デフォルト: バッククォート span 内のマーカーを数えない
allowlist_paths       = []     # 完全に除外する gitignore 形式の glob
```

`ignore_in_inline_code = true`(デフォルト)は、シングル / ダブルバッククォート span の内側にある `TODO` / `FIXME` / `XXX` / `TBD` / `[要確認]` / `[要修正]` のマーカー言及をカウント対象から外します。マーカーキーワード自体を説明するリファレンス(オブザーバの説明、`TODO` の意味を解説するスタイルガイド)は、アクションアイテムを記録しているのではなく単語を引用しているだけなので、デフォルトでオブザーバを無効化せずにこれらだけ除外します。チームがインラインコード span を本当のアクションアイテムとして使っているなら `false` に倒してください。

`allowlist_paths` はマッチした doc を丸ごとスキップします。引用パターンがページ全体に及び、行単位の strip では足りない場合(例: 全マーカー形を本文に列挙する metric リファレンス)に有効です:

```toml
[features.docs.todo_density]
allowlist_paths = [
  "docs/reference/**/metrics.md",
]
```

どちらの knob もカウントから Severity への変換しきい値(3 = Medium、10 = High)は変更しません。

## `.heal/doc_pairs.json` — ペアファイル

ペアファイルは `config.toml` と `calibration.toml` と並んで **git に追跡** されるので、同じコミット上のチームメイトは同じペアを共有します。heal は自動生成しません。

```json
{
  "version": 1,
  "pairs": [
    {
      "doc": "docs/architecture.md",
      "srcs": ["src/lib.rs", "src/observer/mod.rs"],
      "confidence": 0.92,
      "source": "mention"
    },
    {
      "doc": "docs/payments.md",
      "srcs": ["src/payments/engine.ts"],
      "confidence": 1.0,
      "source": "manual"
    }
  ]
}
```

| フィールド | 意味 |
|---|---|
| `version` | スキーマバージョン(現在 `1`)。 |
| `pairs[].doc` | ドキュメントファイルへのプロジェクト相対パス。 |
| `pairs[].srcs` | ドキュメントが説明する 1 つ以上のソースファイル。 |
| `pairs[].confidence` | `0.0` – `1.0`。手動エントリは通常 `1.0`、自動検出は heuristic の confidence。 |
| `pairs[].source` | `"mention"`(ドキュメントが src を参照)、`"mirror"`(ディレクトリ構造ミラー)、`"llm"`(LLM 推論)、`"manual"`(ユーザ作成、再生成で保持)のいずれか。 |

**手動エントリは保持されます。** `/heal-doc-pair-setup` がファイルを再生成するとき、`source: "manual"` の行はそのまま残り、自動検出された行だけが再計算されます。

完全性チェックはベストエフォートです:

- doc パスがディスク上にない → `doc_coverage` Finding として表面化。
- src パスがディスク上にない → `doc_drift` Finding として表面化(ドキュメントが、もう存在しない識別子を参照している状態)。

## Markdown / RST 重複ウィンドウ

`[features.docs]` を有効にすると、Duplication オブザーバが Markdown / RST ファイルに対する並列パスを追加します。ウィンドウ長は `[features.docs]` ではなく `[metrics.duplication]` で調整します(基底のオブザーバが `Duplication` だから):

```toml
[metrics.duplication]
docs_min_tokens = 100        # Markdown / RST のウィンドウ長
```

- `docs_min_tokens`(デフォルト `100`) — Markdown / RST パスの最小ウィンドウ長。トークン化はコードパスとは異なります(word-split + lowercase 化、fenced コードブロックは剥がす)。

## 厳密設計

`[features.docs]` とその子も、他のセクションと同じく未知のキーを拒否します:

```toml
[features.docs.standalone]
includes = ["**/*.md"]   # ✘ unknown — heal はここでエラー
                          #   (正しくは単数形 `include`)
```
