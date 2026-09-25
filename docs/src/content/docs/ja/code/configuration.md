---
title: Code · 設定
description: 常時オンの Code オブザーバファミリの設定方法。コードのメトリクスに関わる .heal/config.toml のすべてのキーと、現実的な既定値。
---

`heal init` は `.heal/` の下に 2 つの TOML ファイルを書き出します。

- `config.toml` — すべてのオブザーバのオン・オフと調整項目。自由に編集してください。
- `calibration.toml` — コードベースを基準にしたパーセンタイルのしきい値。`heal init` と `heal calibrate` が自動で生成します。手では編集しないでください。`floor_critical` / `floor_ok` の上書きは `config.toml` に書けば、calibration をやり直しても消えません。

どちらのファイルもリポジトリごとのもので、全体共通の設定はありません。heal は実行のたびに読み直すので、再起動が必要なデーモンもありません。

このページでは、常時オンの **Code** ファミリを扱います。オプトインのファミリについては [Test › 設定](/heal/ja/test/configuration/) と [Docs › 設定](/heal/ja/docs/configuration/) を参照してください。

## よくある設定

ほとんどのプロジェクトでは、`heal init` が書く既定値のままで動きます。合わないところだけを書き換えてください。

```toml
[project]
response_language = "Japanese"

[git]
since_days = 90
exclude_paths = ["dist/", "vendor/", "node_modules/", ".cache/"]

[metrics]
top_n = 5

[metrics.hotspot]
weight_complexity = 1.5    # churn より complexity を重く見る

[metrics.lcom]
min_cluster_count = 2
```

## 既定値の一覧

| メトリクス       | 既定                                 |
| ---------------- | ------------------------------------ |
| LOC              | 常に有効（オン・オフの切り替えなし） |
| Churn            | 有効                                 |
| Complexity (CCN) | 有効                                 |
| Cognitive        | 有効                                 |
| Duplication      | 有効                                 |
| Change Coupling  | 有効（symmetric を含む）             |
| Hotspot          | 有効                                 |
| LCOM             | 有効                                 |

メトリクスを止めるには、トップレベルの `[metrics] disabled = [...]` の一覧にその名前を加えます。止めたメトリクスは丸ごと実行されず、その Finding が `heal status` に出ることもありません。名前は snake_case の形（`lcom`、`change_coupling` など）で書きます。`loc` は止められません。ほかのすべてのオブザーバが LOC に頼っているからです。

```toml
[metrics]
disabled = ["lcom"]
```

## `[project]`

```toml
[project]
response_language = "Japanese"
```

- `response_language` — Claude のスキルに渡す、応答の言語の指定。Claude が理解できる値なら何でも使えます（`"Japanese"`、`"日本語"`、`"français"`、`"plain English"` など）。書かなくても構いません。

## `[[project.workspaces]]` — モノレポ

パッケージが 1 つだけのリポジトリなら、この節は読み飛ばしてください。モノレポでは、たいてい各 workspace を**それ自身の**分布で calibration したくなります。5 千行の CLI と、その隣にある 5 万行の API が、同じ複雑度の段階を使うのは不自然です。workspace ごとに 1 回ずつ宣言します。

```toml
[[project.workspaces]]
path = "packages/web"
language = "typescript"

[[project.workspaces]]
path = "packages/api"
language = "typescript"

[[project.workspaces]]
path = "services/worker"
language = "rust"
```

各エントリに書ける項目は次のとおりです。

- `path` — リポジトリのルートからの相対パス（区切りはスラッシュ、先頭に `/` は付けない）。workspace を入れ子にはできません。
- `language`（任意）— 自動で判定した主な言語を上書きします。LOC の判定が外れたときに使います（たとえば、`tests/` の下に JavaScript のテスト用データを大量に抱えた Rust の workspace）。
- `exclude_paths`（任意）— workspace のルートを基準に評価する、gitignore 形式のパターン。`git.exclude_paths` に重ねて効きます。

```toml
[[project.workspaces]]
path = "packages/api"
language = "typescript"
exclude_paths = ["vendor/", "src/generated/**"]
```

### workspace ごとのフロアの上書き

ほかの workspace には触れずに、1 つの workspace だけ絶対値のフロアを厳しくしたり緩めたりできます。

```toml
[[project.workspaces]]
path = "packages/legacy"
language = "typescript"

# 複雑度が高いと分かっている古い領域。移行が終わるまで、
# 卒業の基準を緩めておく。
[project.workspaces.metrics.ccn]
floor_ok = 18
```

メトリクスごとに上書きできるのは、`floor_critical` と `floor_ok` です（対象は `ccn` / `cognitive` / `duplication` / `change_coupling` / `lcom`）。workspace での上書きは、全体の `[metrics.<m>]` での上書きより優先されます。

パーセンタイルの区切りは、workspace ごとに自動で計算されます。`[[project.workspaces]]` を宣言するだけで十分です。

### workspace をまたぐ coupling

一緒に変更されるペアが 2 つの workspace にまたがっている（モジュールの境界から漏れている）と、heal はそのペアを `change_coupling.cross_workspace` として付け直します。

```toml
[metrics.change_coupling]
cross_workspace = "surface"   # または "hide"
```

- `surface`（既定）— workspace をまたぐペアを Advisory に入れます。シグナルとしては表示しますが、解消キューには入れません。
- `hide` — 完全に取り除きます。共有のスキーマや、あえて一緒に進化させている API のように、結合が意図的なときに使います。

### workspace で出力を絞り込む

```sh
heal status --workspace packages/api          # packages/api の下の Finding だけ
heal status --json --workspace packages/web   # 範囲を絞った JSON
```

## `[git]`

git の履歴をたどるすべてのメトリクス（churn、change coupling、hotspot）が使います。

```toml
[git]
since_days = 90
exclude_paths = ["dist/"]
```

- `since_days`（既定 `90`）— churn と coupling でさかのぼる期間。
- `exclude_paths` — gitignore 形式のパターン。書式はすべて使えます。グロブ（`*`、`**`、`?`、`[abc]`）、ディレクトリだけに一致させる指定（`foo/`）、ルートに固定する指定（`/foo`）、否定（`!keep`）、コメント（`#`）です。

LOC は既定でこの一覧を引き継ぎます。ほかのオブザーバは常にこの一覧に従います。

## `[metrics]`

```toml
[metrics]
top_n = 5
```

- `top_n`（既定 `5`）— 「ワースト N 件」の一覧すべてに使う既定の件数。オブザーバごとに上書きできます。

下のオブザーバごとの節には、共通して次の項目があります。

- `enabled` — オン・オフの切り替え（LOC にはない）。
- `top_n`（任意）— 全体の既定値を上書きする。
- `floor_critical`（任意、対象のメトリクスのみ）— パーセンタイルの区切りより優先される、Severity の絶対値のフロア。
- `floor_ok`（任意、代理指標のメトリクスのみ）— 絶対値の卒業の基準。これを下回るものはすべて `Ok` になる。

### `[metrics.loc]`

```toml
[metrics.loc]
inherit_git_excludes = true
exclude_paths = []
```

- `inherit_git_excludes`（既定 `true`）— `git.exclude_paths` とあわせて使う。
- `exclude_paths` — LOC だけに効く gitignore 形式のパターン。

### `[metrics.churn]`

```toml
[metrics.churn]
top_n = 10
```

期間の長さは `git.since_days` で決まります。Churn を丸ごと止めるときは、節ごとのフラグではなく `[metrics] disabled = ["churn", ...]` を使います。

### `[metrics.ccn]` と `[metrics.cognitive]`

```toml
[metrics.ccn]
floor_critical = 25     # McCabe の「実質的にテストできない」
floor_ok       = 11     # McCabe の「単純でリスクが低い」

[metrics.cognitive]
floor_critical = 50     # SonarQube の Critical の基準
floor_ok       = 8      # Sonar の「要レビュー」しきい値の半分
```

既定値は文献に基づいています。上書きするのは、扱う領域の性質から、もっと厳しい、あるいは緩いしきい値が必要なときだけにしてください。上書きすると、`heal status` はヘッダーの行にそれを表示するので、policy の変更を CI のログで追えます。

### `[metrics.duplication]`

```toml
[metrics.duplication]
min_tokens     = 50
floor_critical = 30      # 30% が重複しているなら構造の問題
```

- `min_tokens`（既定 `50`）— **コード**の重複ブロックとみなす最小のトークン数。小さくすると、短いブロックも見つかります。
- `docs_min_tokens`（既定 `100`）— Markdown / RST の重複検出で使う最小のトークン数。`[features.docs]` が有効なときだけ使います。[Docs › メトリクス](/heal/ja/docs/metrics/#markdown-重複) を参照してください。

### `[metrics.change_coupling]`

```toml
[metrics.change_coupling]
min_coupling        = 3
symmetric_threshold = 0.5
```

- `min_coupling`（既定 `3`）— 一緒に変更された回数がこれより少ないペアは、順位付けの前に捨てます。
- `symmetric_threshold`（既定 `0.5`）— `P(B|A)` と `P(A|B)` の両方がこの値に届いたペアを `Symmetric` に分類します。

### `[metrics.hotspot]`

```toml
[metrics.hotspot]
weight_churn      = 1.0
weight_complexity = 1.0
```

- 合成したスコアは `(weight_complexity × ccn_sum) × (weight_churn × commits)` です。両方の重みが正なら、どちらを変えてもすべてのスコアが同じ倍率で伸び縮みするだけで、順位は変わりません。重みを `0.0` にすると、その側を合成から外せます。

Hotspot には `floor_critical` がありません。Severity の段階ではなく、フラグだからです。有限の候補が 5 件以上あれば、スコアは p90 とファミリのフロアの両方を超える必要があります。候補が 1〜4 件のときは、ファミリの絶対値のフロア（Code 22、Test 25、Docs 5）だけを使います。有限でないスコアはフラグの対象になりません。

### `[metrics.lcom]`

```toml
[metrics.lcom]
min_cluster_count = 2
```

- `min_cluster_count`（既定 `2`）— クラスタの数がこれより少ないクラスは、Severity を決める前に捨てます。`2` が自然な基準です（機械的に分けられるクラス）。

## `[diff]`

`heal diff` の、作業ツリーを使う動作を調整します。

```toml
[diff]
max_loc_threshold = 200_000
```

- `max_loc_threshold`（既定 `200_000`）— 別の参照を走査するために、一時的な `git worktree` を作ってよい LOC の合計の上限。これを超えると、`heal diff <ref>` は worktree を作らずに、2 つのブランチを手動で比べる手順を表示して、終了コード 2 で終わります。

## `[policy.drain]`

解消ポリシーは、どの `(Severity, hotspot)` の組み合わせを必ず解消するか（T0）、手が空いたときに解消してよいか（T1）を決めます。`/heal:refactor` は T0 から先に提案します。どちらの一覧にも入らないものは Advisory になり、`--all` を付けたときだけ表示されます。

```toml
[policy.drain]
must   = ["critical:hotspot"]            # T0 — ゼロになるまで解消する
should = ["critical", "high:hotspot"]    # T1 — 都合のよいときに解消する
```

書き方は次のとおりです。

- `<severity>` — その Severity に一致する。Hotspot かどうかは問わない。
- `<severity>:hotspot` — その Severity で、かつ `hotspot = true` のものに一致する。

Severity は小文字で `critical`、`high`、`medium`、`ok` と書きます。知らない値があると、設定を読み込む時点でエラーになります。

## `.heal/calibration.toml`

`heal init` が生成し、`heal calibrate --force` で作り直します。手では編集しないでください。`floor_critical` と `floor_ok` は、calibration をやり直しても消えないように `config.toml` に書きます。

```toml
[meta]
created_at         = "2026-04-30T09:00:00Z"
codebase_files     = 142
calibrated_at_sha  = "a0a6d1a7f3…"
strategy           = "percentile"

[calibration.ccn]
p50 = 4.2
p75 = 8.1
p90 = 14.3
p95 = 21.7
floor_critical = 25.0
floor_ok       = 11.0

[calibration.hotspot]
p50 = 5.0
p75 = 18.0
p90 = 67.0          # 候補が 5 件以上あるときの、Hotspot 🔥 のパーセンタイルの基準
p95 = 145.0
```

`heal calibrate`（フラグなし）は、ファイルがないときだけ作ります。すでにあれば、あることを報告するだけで何も書き換えません。実際に走査し直すには `--force` を付けます。コードベースが十分に変わると `heal doctor` が知らせ、`/heal:setup` が `heal calibrate --force` を勧めます。

## 厳密な設計

どの節も、定義されていないキーがあるとエラーにします。綴りを間違えた項目は黙って無視されず、起動時に読み込みエラーになります。

```toml
[metrics]
typo_n = 5     # ✘ 知らない項目 — heal はここでエラーを出す
```

エラーにはファイルのパスと行番号が入ります。黙って無視されると、設定の間違いがそのまま本番まで届きやすいからです。間違いにはすぐ気づけるほうがいい、という考えです。
