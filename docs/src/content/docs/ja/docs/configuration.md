---
title: Docs · 設定
description: '[features.docs] の有効にしかた、走査する standalone のドキュメントの選び方、鮮度のフロアの調整、scaffold の出力先の設定、.heal/doc_pairs.json の中身。'
---

**Docs** ファミリはオプトインで、既定では無効です。コードのメトリクスと並べて、古くなったドキュメントも見たくなったら有効にしてください。外部の HTTP リンクの確認と、サンプルコードの実行は扱いません。heal はローカルだけで動くツールで、HTTP の側は CI で `lychee` や `linkchecker` が受け持ちます。

各メトリクスが何を見つけるかは [Docs › メトリクス](/heal/ja/docs/metrics/) を、スキルについては [Docs › スキル](/heal/ja/docs/skills/) を参照してください。

## すぐに有効にする

```toml
[features.docs]
enabled = true
```

そのあと、セットアップ用スキルの docs の手順を 1 度実行して、`.heal/doc_pairs.json` を作ってください（ファミリの有効化もあわせて行えます）。heal はこのファイルを読むだけです。詳しくは下の [`.heal/doc_pairs.json`](#healdoc_pairsjson--ペアのファイル) を参照してください。Claude Code で次を実行します。

```
/heal:setup docs
```

## `[features.docs]`

```toml
[features.docs]
enabled       = false                        # ファミリ全体のスイッチ
pairs_path    = ".heal/doc_pairs.json"       # 対応表の置き場所
scaffold_root = ".heal/docs"                 # /heal:docs scaffold の出力先
```

- `enabled`（既定 `false`）— ファミリ全体のスイッチ。false のあいだ、docs のオブザーバはどれも何もせず、`.heal/doc_pairs.json` も読みません。
- `pairs_path`（既定 `.heal/doc_pairs.json`）— ペアのファイルの、プロジェクトからの相対パス。heal は読むだけで、作るのは `/heal:setup` スキルの役目です。
- `scaffold_root`（既定 `.heal/docs`）— `/heal:docs scaffold` が Markdown のひな形を書き出す、プロジェクトからの相対パス。heal 自身はこのツリーを読みも書きもしません。この項目は、チームの誰が scaffold を作り直しても同じ場所に出力されるようにするための情報です。既定では `.heal/` の下に出力するので、プロジェクトにすでにある `docs/` ディレクトリとはぶつかりません。ひな形を見直したら `git mv .heal/docs docs` で移し、`scaffold_root = "docs"` にしてください。次に作り直すときは、公開している場所に直接書き出されます。

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

`standalone` は **Layer B** のドキュメントを扱います。README、コンセプトの説明、解説ページのような文章のページで、リンク・孤立ページ・TODO の確認は必要でも、ソースとのペアは要らないものです。

既定の `exclude` で外しているのは、次のものです。

- 運営や履歴のファイル（`CHANGELOG*`、`CONTRIBUTING*`、`CODE_OF_CONDUCT*`、`SECURITY*`）。日付の入った履歴に、ずれの検出は当てはまりません。
- ADR（`**/adr/**`）。各エントリには日付が入り、マージした後は編集しないのが慣習です。
- 生成された API リファレンスとビルドの成果物。

既定で拾えない生成済みのドキュメント（たとえば `docs/api-generated/` のツリー）があれば、`exclude` に加えてください。

## `[features.docs.doc_freshness]`

```toml
[features.docs.doc_freshness]
high_commits     = 5    # ドキュメントより後のソースのコミット数 → High
critical_commits = 20   # ドキュメントより後のソースのコミット数 → Critical
```

コミットの数で測る、絶対値のフロアです。距離をコミット数で数えるので、チームのコミットのペースが変わっても、しきい値はずれません。

判定のしかたは次のとおりです。

| `src_commits_since_doc ≥` | Severity |
| ------------------------- | -------- |
| `critical_commits`        | Critical |
| `high_commits`            | High     |
| 1                         | Medium   |

厳しくするなら両方のフロアを下げ、緩めるなら上げます。

## `[features.docs.todo_density]`

```toml
[features.docs.todo_density]
ignore_in_inline_code = true   # 既定: `…` の中の印は数えない
allowlist_paths       = []     # 丸ごと飛ばす、gitignore 形式のグロブ
```

`ignore_in_inline_code = true`（既定）にしておくと、バッククォート 1 つまたは 2 つで囲んだ範囲の*中*にある `TODO` / `FIXME` / `XXX` / `TBD` / `[要確認]` / `[要修正]` は数えません。印のキーワードそのものを説明するリファレンスのページ（オブザーバの説明や、`TODO` の意味を解説したスタイルガイド）は、印の言葉を引用しているだけです。既定値は、オブザーバをプロジェクト全体で止めずに、そうした引用を数えないようにします。チームがインラインコードの中に本物の作業項目を書いているなら、`false` にしてください。

`allowlist_paths` に一致するドキュメントは、丸ごと飛ばします。ページ*全体*が引用でできていて、行ごとに取り除くだけでは足りないときに使います（たとえば、本文ですべての印の形を並べているメトリクスのリファレンス）。

```toml
[features.docs.todo_density]
allowlist_paths = [
  "docs/reference/**/metrics.md",
]
```

どちらの項目も、件数から Severity を決めるフロア（3 件で Medium、10 件で High）には影響しません。

## `[features.docs.doc_link_health]`

```toml
[features.docs.doc_link_health]
exclude_link_prefixes = []   # 既定: 内部リンクをすべてソースのツリーと照らし合わせる
```

`exclude_link_prefixes` に挙げた前置きで始まるリンクは、ソースのツリーとの照合から外します。そのリンクは、解決できたとも壊れているとも数えず、照合そのものを素通りします。ビルド時にフレームワークが書き換える、静的サイトの公開用の URL に使います。

```toml
[features.docs.doc_link_health]
exclude_link_prefixes = ["/heal/"]   # Starlight の base: '/heal'
```

| フレームワーク         | フレームワーク側の設定       | `exclude_link_prefixes` の値 |
| ---------------------- | ---------------------------- | ---------------------------- |
| Astro Starlight        | `base: '/heal'`              | `["/heal/"]`                 |
| VitePress / Docusaurus | `base: '/docs/'`             | `["/docs/"]`                 |
| mdBook                 | `book.url-prefix = "/guide"` | `["/guide/"]`                |

これらのリンク先は、フレームワーク自身のビルド時のリンクチェック（`astro build` など）が公開する側から確かめているので、heal がその部分を任せても見落としは生まれません。空の値（`""`）は無視します。空文字 1 つでオブザーバ全体が黙ってしまう、という事故を防ぐためです。

## `.heal/doc_pairs.json` — ペアのファイル

ペアのファイルは、`config.toml` や `calibration.toml` と同じく **git で管理します**。同じコミットにいるチームメンバーが、同じペアの一覧を見るためです。heal がこのファイルを自動で作ることはありません。

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

| フィールド           | 意味                                                                                                                                                                                   |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `version`            | スキーマのバージョン（今は `1`）。                                                                                                                                                     |
| `pairs[].doc`        | ドキュメントのファイルの、プロジェクトからの相対パス。                                                                                                                                 |
| `pairs[].srcs`       | そのドキュメントが説明するソースファイル（1 つ以上）。                                                                                                                                 |
| `pairs[].confidence` | `0.0` 〜 `1.0`。手で書いたエントリはたいてい `1.0`、自動で見つけたエントリには判定の確信度が入る。                                                                                     |
| `pairs[].source`     | `"mention"`（ドキュメントがソースに言及している）、`"mirror"`（ディレクトリの構成が対応している）、`"llm"`（LLM が推測した）、`"manual"`（利用者が書いた。作り直しても残る）のどれか。 |

**手で書いたエントリには手を触れません。** `/heal:setup` がファイルを作り直すとき、`source: "manual"` の行はすべてそのまま残します。計算し直すのは、自動で見つけた行だけです。

整合性の確認は、できる範囲で行います。

- ドキュメントのパスがディスクにない → `doc_coverage` の Finding になります。
- ソースのパスがディスクにない → `doc_drift` の Finding になります（ドキュメントが、もう存在しない識別子を参照していることになるため）。

## Markdown / RST の重複検出の窓

`[features.docs]` がオンのとき、Duplication のオブザーバは Markdown / RST のファイルにも同じ検出を並行して行います。窓の長さは `[metrics.duplication]` で調整します。`[features.docs]` の下に置いていないのは、もとになっているオブザーバが `Duplication` だからです。

```toml
[metrics.duplication]
docs_min_tokens = 100        # Markdown / RST の窓
```

- `docs_min_tokens`（既定 `100`）— Markdown / RST の検出で使う、窓の最小の長さ。トークンの区切り方はコードとは違い、単語で区切って小文字にし、フェンスで囲んだコードブロックは取り除きます。

## 厳密な設計

ほかの節と同じく、`[features.docs]` とその下の節も、定義されていないキーがあるとエラーにします。

```toml
[features.docs.standalone]
includes = ["**/*.md"]   # ✘ 知らない項目 — heal はここでエラーを出す
                          #   （正しくは単数形の `include`）
```
