---
title: Code · 設定
description: 常時オンの Code オブザーバファミリを設定する .heal/config.toml の各キーと、現実的なデフォルト。
---

`heal init` は `.heal/` の下に 2 つの TOML を書き出します:

- `config.toml` — オブザーバの切り替えと調整値。手で自由に編集できます。
- `calibration.toml` — コードベース相対のパーセンタイル閾値。`heal init` と `heal calibrate` が自動生成します。手編集はせず、`floor_critical` / `floor_ok` の上書きは `config.toml` に置いてください。再 calibrationで消えないようにするためです。

どちらもリポジトリごとのファイルです。グローバル設定はありません。heal は呼び出しごとに再読み込みするので、再起動すべきデーモンもありません。

このページは常時オンの **Code** ファミリを扱います。オプトインのファミリは [Test › 設定](/heal/ja/test/configuration/) と [Docs › 設定](/heal/ja/docs/configuration/) を参照。

## 典型的な設定

ほとんどのプロジェクトは `heal init` のデフォルトのままで動きます。合わない部分だけ上書きしてください:

```toml
[project]
response_language = "Japanese"

[git]
since_days = 90
exclude_paths = ["dist/", "vendor/", "node_modules/", ".cache/"]

[metrics]
top_n = 5

[metrics.hotspot]
weight_complexity = 1.5    # churn より complexity を重視

[metrics.lcom]
min_cluster_count = 2
```

## デフォルトの一覧

| メトリクス       | デフォルト             |
| ---------------- | ---------------------- |
| LOC              | 常に有効(切り替え不可) |
| Churn            | 有効                   |
| Complexity (CCN) | 有効                   |
| Cognitive        | 有効                   |
| Duplication      | 有効                   |
| Change Coupling  | 有効(symmetric を含む) |
| Hotspot          | 有効                   |
| LCOM             | 有効                   |

無効化するには最上位の `[metrics] disabled = [...]` リストにメトリクス名を追加します。名前は snake_case 形式(`lcom`、`change_coupling` など)。`loc` は他の全オブザーバが依存するため無効化できません。無効化したメトリクスは完全にスキップされ、`heal status` にも現れません。

```toml
[metrics]
disabled = ["lcom"]
```

## `[project]`

```toml
[project]
response_language = "Japanese"
```

- `response_language` — Claude スキルに渡す言語ヒント。`"Japanese"`、`"日本語"`、`"français"`、`"plain English"` など Claude が理解できる文字列なら何でも構いません。任意項目です。

## `[[project.workspaces]]` — モノレポ

単一パッケージのリポジトリではこのセクションは不要です。モノレポでは、各ワークスペースを **それぞれの分布** でcalibrate したほうが現実に合います。5kloc の CLI と 50kloc の API が同じ複雑度ラダーを共有する必要はありません。各ワークスペースを宣言すると、heal はそれぞれのファイルを独立して分類します。

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

各エントリで指定できる項目:

- `path` — リポジトリルートからの相対ディレクトリ(スラッシュ区切り、先頭の `/` なし)。ワークスペースはネストできません。
- `language`(任意) — 自動検出された主言語を上書きします。LOC のヒューリスティックが想定と違う言語を選ぶとき(例: Rust ワークスペースに JavaScript のフィクスチャが大量にある場合)に使います。
- `exclude_paths`(任意) — ワークスペースルートからの相対で評価される gitignore 構文のパターン。`git.exclude_paths` の上に重ねて適用されます。

```toml
[[project.workspaces]]
path = "packages/api"
language = "typescript"
exclude_paths = ["vendor/", "src/generated/**"]
```

### ワークスペースごとのフロア上書き

特定のワークスペースだけ絶対フロアを締めたり緩めたりできます:

```toml
[[project.workspaces]]
path = "packages/legacy"
language = "typescript"

# 高複雑度のレガシーエリア。移行が完了するまで卒業ゲートを緩めておく。
[project.workspaces.metrics.ccn]
floor_ok = 18
```

メトリクスごとに指定できる上書きは `floor_critical` と `floor_ok` です(対象は `ccn` / `cognitive` / `duplication` / `change_coupling` / `lcom`)。ワークスペース上書きはグローバルな `[metrics.<m>]` 上書きに勝ちます。

パーセンタイル区切りはワークスペースごとに自動計算されます。`[[project.workspaces]]` を宣言するだけで十分です。

### ワークスペースをまたぐ coupling

co-change のペアが 2 つのワークスペースをまたぐ場合(モジュール境界の漏れ)、heal はそのペアを `change_coupling.cross_workspace` と再タグします:

```toml
[metrics.change_coupling]
cross_workspace = "surface"   # または "hide"
```

- `surface`(デフォルト) — Advisory バケットに送ります。シグナルとして表示はされますが 解消キューには入りません。
- `hide` — 完全に捨てます。共有スキーマや意図的に co-evolve させている API のように、coupling が意図的なときに使います。

### ワークスペースで出力をフィルタ

```sh
heal status --workspace packages/api          # packages/api 配下の findings のみ
heal status --json --workspace packages/web   # JSON、スコープ付き
```

## `[git]`

git 履歴を歩くすべてのメトリクス(churn、change coupling、hotspot)が参照します。

```toml
[git]
since_days = 90
exclude_paths = ["dist/"]
```

- `since_days`(デフォルト `90`) — churn / coupling の参照ウィンドウです。
- `exclude_paths` — gitignore 構文のパターン。グロブ(`*`、`**`、`?`、`[abc]`)、ディレクトリのみ(`foo/`)、ルートアンカー(`/foo`)、否定(`!keep`)、コメント(`#`)の DSL がそのまま使えます。

LOC はデフォルトでこのリストを継承します。他のオブザーバは常に尊重します。

## `[metrics]`

```toml
[metrics]
top_n = 5
```

- `top_n`(デフォルト `5`) — すべての「ワースト N」リストのデフォルトサイズ。各オブザーバは個別に上書きできます。

各サブセクションが共通で持つ項目:

- `enabled` — マスタートグル(LOC にはありません)。
- `top_n`(任意) — グローバルデフォルトの上書き。
- `floor_critical`(該当する場合) — パーセンタイルに勝つ絶対 Severity フロア。
- `floor_ok`(プロキシメトリクスのみ) — 絶対卒業ゲート。これより厳密に下なら percentile に関係なく `Ok` です。

### `[metrics.loc]`

```toml
[metrics.loc]
inherit_git_excludes = true
exclude_paths = []
```

- `inherit_git_excludes`(デフォルト `true`) — `git.exclude_paths` と組み合わせます。
- `exclude_paths` — LOC 限定の gitignore 構文パターン。

### `[metrics.churn]`

```toml
[metrics.churn]
top_n = 10
```

ウィンドウ長は `git.since_days` を使います。Churn 自体を無効化したい場合は最上位の `[metrics] disabled = ["churn", ...]` を使います(セクション内のフラグではありません)。

### `[metrics.ccn]` と `[metrics.cognitive]`

```toml
[metrics.ccn]
floor_critical = 25     # McCabe 「実質的にテスト不能」
floor_ok       = 11     # McCabe 「単純、低リスク」

[metrics.cognitive]
floor_critical = 50     # SonarQube Critical の基準
floor_ok       = 8      # Sonar の review 閾値の半分
```

デフォルトは文献由来です。ドメイン側で厳しめ / 緩めの閾値が必要なときだけ上書きします。上書きすると `heal status` がヘッダ行で「`override: ccn floor_ok=15 [override from 11]`」のように表示するので、ポリシー変更が CI ログから追えます。

### `[metrics.duplication]`

```toml
[metrics.duplication]
min_tokens     = 50
floor_critical = 30      # 30% が重複なら構造的問題
```

- `min_tokens`(デフォルト `50`) — **コード**側の重複ブロックの最小ウィンドウ長。下げるほど短くて多いブロックを拾います。
- `docs_min_tokens`(デフォルト `100`) — Markdown / RST の重複パスの最小ウィンドウ長。`[features.docs]` が有効なときだけ参照されます。詳しくは [Docs › Metrics](/heal/ja/docs/metrics/#markdown-重複)。

### `[metrics.change_coupling]`

```toml
[metrics.change_coupling]
min_coupling        = 3
symmetric_threshold = 0.5
```

- `min_coupling`(デフォルト `3`) — co-change 回数がこれ未満のペアは捨てます。
- `symmetric_threshold`(デフォルト `0.5`) — `P(B|A)` と `P(A|B)` の両方がこれ以上のペアを `Symmetric` と分類します。

### `[metrics.hotspot]`

```toml
[metrics.hotspot]
weight_churn      = 1.0
weight_complexity = 1.0
```

- 合成スコアは `(weight_complexity × ccn_sum) × (weight_churn × commits)`。どちらかを `0.0` にすると、その側を合成から外せます(オブザーバ自体の無効化ではありません)。

Hotspot には `floor_critical` がありません。Severity ティアではなくフラグ(スコア分布の上位 10%)です。

### `[metrics.lcom]`

```toml
[metrics.lcom]
min_cluster_count = 2
```

- `min_cluster_count`(デフォルト `2`) — クラスタ数がこのフロア未満のクラスは Severity 分類前に捨てます。`2` が自然なベースライン(クラスが機械的に分割可能)です。

## `[diff]`

`heal diff` の worktree モードを調整します。

```toml
[diff]
max_loc_threshold = 200_000
```

- `max_loc_threshold`(デフォルト `200_000`) — 別の ref を走査するための一時的な `git worktree` を作る LOC 上限。これを超えると `heal diff <ref>` は終了コード 2 で抜け、worktree クローンの代わりに手動 2 ブランチ手順を表示します。

## `[policy.drain]`

解消ポリシーが、`/heal-code-patch` が必ず解消する `(Severity, hotspot)` の組み合わせ(T0)と、帯域に余裕があれば解消する組み合わせ(T1)を決めます。両方のリストの外にあるものは Advisory に落ち、`--all` でしか表示されません。

```toml
[policy.drain]
must   = ["critical:hotspot"]            # T0 — ゼロまで解消
should = ["critical", "high:hotspot"]    # T1 — 余裕があれば
```

DSL 文法:

- `<severity>` — その severity にマッチ(hotspot は問わない)。
- `<severity>:hotspot` — その severity かつ `hotspot = true` にマッチ。

Severity トークンは小文字: `critical`、`high`、`medium`、`ok`。未知のトークンは config 読み込み時にエラーになります。

## `.heal/calibration.toml`

`heal init` が生成し、`heal calibrate --force` が更新します。手編集は避け、`floor_critical` と `floor_ok` の上書きは `config.toml` 側に置いてください(再 calibrationで消えないようにするため)。

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
p90 = 67.0          # Hotspot 🔥 のフラグ境界(上位 10%、固定)
p95 = 145.0
```

`heal calibrate`(フラグなし)はファイルがないときだけ作成します。既にあるときは存在を報告するだけで何も書き換えません。実際に再走査するには `--force` を渡してください。`/heal-setup` スキルがドリフトを監視し、必要に応じて `heal calibrate --force` を提案します。

## 厳密設計

すべてのセクションで未知のキーを拒否します。タイプミスは silent drop ではなく、起動時の parse エラーとして表面化します:

```toml
[metrics]
typo_n = 5     # ✘ unknown field — heal はここでエラー
```

エラーメッセージにはファイルパスと行番号が含まれます。silent drop は config の誤りが本番に届くよくある経路なので、すぐ気づけるようにしています。
