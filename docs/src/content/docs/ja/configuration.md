---
title: 設定
description: .heal/config.toml と .heal/calibration.toml の読み方と編集方法 — 全セクションを解説、現実的な例つき。
---

`heal init` は `.heal/` 以下に 2 つの TOML ファイルを書き出します。

- `config.toml` — 各オブザーバーのトグルとチューニング値。自由に
  編集して構いません。
- `calibration.toml` — コードベース相対のパーセンタイルしきい値。
  `heal init` / `heal calibrate` が自動生成します。**手で編集する
  ことは想定していません**。`floor_critical` の上書きは `config.toml`
  に書きます。

両ファイルともリポジトリ単位で、グローバル設定はありません。heal は
呼び出しごとにファイルを読み直すため、再起動が必要なデーモンもあり
ません。

## おすすめのスタート地点

ほとんどのプロジェクトでは、`heal init` のデフォルトが良い出発点に
なります。プロジェクトに合わない部分だけ上書きしてください。典型的
な設定:

```toml
[project]
# Claude スキルに渡される自由記述の言語ヒント。
response_language = "Japanese"

[git]
since_days = 90
exclude_paths = ["dist/", "vendor/", "node_modules/", ".cache/"]

[metrics]
top_n = 5

[metrics.change_coupling]
enabled = true
min_coupling = 3
symmetric_threshold = 0.5

[metrics.hotspot]
enabled = true
weight_churn = 1.0
weight_complexity = 1.5

[metrics.lcom]
enabled = true
backend = "tree-sitter-approx"
min_cluster_count = 2
```

## デフォルト

`heal init` 後、すべてのオブザーバーがデフォルトで有効です。無効化
するには対応セクションで `enabled = false` を設定してください。

| メトリクス       | デフォルト                   |
| ---------------- | ---------------------------- |
| LOC              | 常に有効（トグルなし）       |
| Churn            | 有効                         |
| Complexity (CCN) | 有効                         |
| Cognitive        | 有効                         |
| Duplication      | 有効                         |
| Change Coupling  | 有効（symmetric を含む）     |
| Hotspot          | 有効                         |
| LCOM             | 有効（`tree-sitter-approx`） |

無効化されたメトリクスは完全にスキップされます。オブザーバーが走ら
ず、`heal status` にも現れません。

## `[project]`

プロジェクトレベルのメタデータ。

```toml
[project]
response_language = "Japanese"
```

- `response_language` — Claude スキルに渡される自由記述の言語ヒント。
  Claude が解釈できる任意の値が使えます: `"Japanese"`、`"日本語"`、
  `"français"`、`"plain English"` など。任意項目です。

## `[[project.workspaces]]` — モノレポ向け

単一パッケージのリポジトリではデフォルトで十分なので、このセクションは飛ばして構いません。モノレポでは通常、各 workspace を **それぞれ自身の** 分布で calibrate したくなります — 5kloc の CLI と隣にある 50kloc の API は、複雑度のラダーを共有しないほうが自然だからです。各 workspace を 1 度宣言すれば、heal はそれぞれを独立に分類します。

```toml
[[project.workspaces]]
path = "packages/web"
primary_language = "typescript"

[[project.workspaces]]
path = "packages/api"
primary_language = "typescript"

[[project.workspaces]]
path = "services/worker"
primary_language = "rust"
```

各エントリの項目:

- `path` — リポジトリルートからの相対ディレクトリ（スラッシュ区切り、先頭に `/` なし、`..` なし）。workspace はネストできません。最長の workspace `path` にマッチしたファイルがその workspace に属し、どの workspace にも属さないファイルは「leftover」コホートとして独自の calibration を持ちます。
- `primary_language`（任意） — そのサブツリーで自動検出される primary language を上書き。LOC のヒューリスティックが間違った言語を選ぶとき（例: Rust workspace に重い `tests/` の JS フィクスチャがある場合）に有用。
- `exclude_paths`（任意） — `.gitignore` 文法のパターンを **workspace ルート相対** で評価し、`git.exclude_paths` と `metrics.loc.exclude_paths` の上に積み増します。先頭 `/` は workspace ルートにアンカーし、`!pat` は前の除外を打ち消し、`.gitignore` と同じグロブ（`*`、`**`、`?`、`[abc]`）が使えます。

```toml
[[project.workspaces]]
path = "packages/api"
primary_language = "typescript"
exclude_paths = ["vendor/", "src/generated/**"]
```

### workspace ごとのフロア上書き

特定の workspace だけ絶対フロアを厳しく／緩くできます。`[metrics.<m>]` と同じ形でスコープが workspace になります。

```toml
[[project.workspaces]]
path = "packages/legacy"
primary_language = "typescript"

# この workspace は高複雑度なのが分かっているので、移行作業中は卒業
# ゲートを緩めて CCN finding の山に埋もれないようにする。
[project.workspaces.metrics.ccn]
floor_ok = 18
```

利用できるフィールドはメトリクスごとに `floor_critical` と `floor_ok` です（`ccn` / `cognitive` / `duplication` / `change_coupling` / `lcom`）。workspace の上書きはグローバルの `[metrics.<m>]` *の後* に適用されるため、両方ある場合は workspace の値が勝ちます。

パーセンタイル区切り（p75 / p90 / p95）も workspace ごとに計算されます — `heal calibrate` は宣言された workspace ごとに `[calibration.workspaces."<path>".<metric>]` の表を 1 つずつ記録します。手動セットアップは不要で、`[[project.workspaces]]` を宣言するだけで十分です。

### workspace 間の coupling

`change_coupling` が有効で、共変するペアが 2 つの workspace にまたがる場合（「モジュール境界リーク」）、heal はそのペアを `change_coupling.cross_workspace` に retag します。

```toml
[metrics.change_coupling]
enabled         = true
cross_workspace = "surface"   # または "hide"
```

- `surface`（デフォルト） — workspace 間ペアは専用の Advisory バケットに入ります。シグナルとしては見えますが drain queue には入りません（これはリファクタではなくアーキテクチャ判断だからです）。
- `hide` — 完全に落とします。共有スキーマや意図的に co-evolve する API など、その coupling が意図的なときに使います。

どちらも `[[project.workspaces]]` が空のときは無視されます。

### workspace で出力を絞る

`heal status` は `--workspace <path>` で表示を 1 つの workspace に絞れます。`--json` では `workspaces` 配列が workspace ごとの `severity_counts` を持つので、スクリプトから扱えます。

```sh
heal status --workspace packages/api          # API の finding のみ表示
heal status --json --workspace packages/web   # JSON でスコープ
```

## `[git]`

git の履歴を辿るメトリクス（Churn、Change Coupling、Hotspot）すべ
てが参照します。

```toml
[git]
since_days = 90
exclude_paths = ["dist/"]
```

- `since_days`（デフォルト `90`） — Churn / Coupling の参照ウィン
  ドウ。
- `exclude_paths`（デフォルト `[]`） — 無視する**パス部分文字列**の
  リスト。`"dist/"` は `packages/web/dist/foo.js` と
  `apps/api/dist/bar.js` の両方にマッチします。グロブパターンは未
  対応です。

LOC オブザーバーはデフォルトでこのリストを継承します。他のオブザー
バーは常に尊重します。

## `[metrics]`

トップレベルのフィールドはオブザーバー間で共有されます。

```toml
[metrics]
top_n = 5
```

- `top_n`（デフォルト `5`） — すべての「worst-N」表示のデフォルト
  サイズ。各オブザーバー個別の `top_n` で上書きできます。

以下の各オブザーバーサブセクションには共通して次の項目があります。

- `enabled` — マスター切り替え（LOC は常時有効）。
- `top_n`（任意） — そのオブザーバーのランキング用にグローバルデフォ
  ルトを上書き。
- `floor_critical`（該当する場合のみ、任意） — パーセンタイル区切
  りに勝つ Severity の絶対フロア。コードベース分布が緩めでも文献基
  準のしきい値を効かせたいときに有用。
- `floor_ok`（proxy メトリクスのみ、任意） — 絶対の卒業ゲート。値が
  これ未満の Finding はパーセンタイルに関係なく `Ok` に分類されま
  す。クリーンになったコードベースが「上位 10% は永遠に赤」のループ
  から抜けられるようにする仕組み。デフォルトは `ccn = 11`（McCabe
  "simple"）、`cognitive = 8`（Sonar）。

### `[metrics.loc]`

```toml
[metrics.loc]
inherit_git_excludes = true
exclude_paths = []
```

- `inherit_git_excludes`（デフォルト `true`） — `git.exclude_paths`
  と統合します。`false` で切り離します。
- `exclude_paths` — LOC 専用のパス部分文字列。

### `[metrics.churn]`

```toml
[metrics.churn]
enabled = true
```

ウィンドウ長は `git.since_days` から取られます。

### `[metrics.ccn]` と `[metrics.cognitive]`

```toml
[metrics.ccn]
enabled        = true
floor_critical = 25     # McCabe "untestable in practice"
floor_ok       = 11     # McCabe "simple, low risk" — 卒業ゲート

[metrics.cognitive]
enabled        = true
floor_critical = 50     # SonarQube Critical のベースライン
floor_ok       = 8      # Sonar "review" 閾値の半分
```

- `floor_critical` — Severity の絶対フロア。`≥ floor` のものはパー
  センタイル位置に関係なく Critical に分類されるため、コードベース
  全体が悪い場合でもワーストケースを表面化できます。
- `floor_ok` — 絶対の卒業ゲート。値が `< floor_ok` のものはパーセン
  タイルに関係なく `Ok` に分類されます。これにより、コードベース全
  体がクリーンになっても「上位 10% は永遠に Critical」のループ
  （Goodhart's Law）から抜けられます。デフォルトは文献由来。チーム
  ごとのドメインがより厳しい/緩い閾値を求める場合に上書きします。
  Override の可視化: `heal check` のヘッダ行に
  `override: ccn floor_ok=15 [override from 11]` のような注釈が出
  力され、CI ログで policy 変更が監査できます。

### `[metrics.duplication]`

```toml
[metrics.duplication]
enabled        = true
min_tokens     = 50
floor_critical = 30      # 30% 重複は構造的な問題
```

- `min_tokens`（デフォルト `50`） — 重複ブロックの最小ウィンドウ長。
  値を下げるとより多く・より短いブロックが拾われます。

### `[metrics.change_coupling]`

```toml
[metrics.change_coupling]
enabled             = true
min_coupling        = 3
symmetric_threshold = 0.5
```

- `min_coupling`（デフォルト `3`） — この回数より少なく共変したペ
  アはランキング前にドロップされます。
- `symmetric_threshold`（デフォルト `0.5`） — `P(B|A)` と `P(A|B)`
  の両方がこの値を満たすと、そのペアは `Symmetric` に分類されます。
  しきい値未満の場合は `OneWay` になり、partner により条件依存が強
  い側がリーダーに指名されます。

### `[metrics.hotspot]`

```toml
[metrics.hotspot]
enabled           = true
weight_churn      = 1.0
weight_complexity = 1.0
```

- `weight_churn` と `weight_complexity`（どちらもデフォルト `1.0`）
  — 合成スコアは `(weight_complexity × ccn_sum) × (weight_churn ×
commits)`。どちらかを `0.0` にすると、その側の合成だけ無効化され
  ます（オブザーバー本体は無効化されません）。

Hotspot 自体に `floor_critical` はありません。Severity Tier ではな
くフラグ（スコア分布の上位 10%）であるためです。

### `[metrics.lcom]`

```toml
[metrics.lcom]
enabled           = true
backend           = "tree-sitter-approx"
min_cluster_count = 2
```

- `backend` — 抽出戦略。`"tree-sitter-approx"` は v0.2 のデフォル
  ト（型情報なしの純粋な構文ウォーク）。`"lsp"` は v0.5+ 用に予約
  済み。未知の値はパースに失敗します。
- `min_cluster_count`（デフォルト `2`） — `cluster_count` がこの値
  未満のクラスは Severity 分類の前にドロップされます。`2` が自然な
  ベースラインです（クラスが機械的に分割可能）。

## `[diff]`

`heal diff` の worktree モード（要求された git ref が
`latest.json` と一致しない場合のフォールバック）を調整します。

```toml
[diff]
max_loc_threshold = 200_000
```

- `max_loc_threshold`（デフォルト `200_000`） — 別の ref をスキャ
  ンするために `git worktree` を一時展開する LOC 上限。閾値を超え
  ると `heal diff <git-ref>` は終了コード 2 で終了し、worktree を
  作る代わりに 2 ブランチを並べて確認する手順を表示します。

## `.heal/calibration.toml`

`heal init` が生成し、`heal calibrate --force` で更新します。手編
集は推奨されません — `floor_critical` / `floor_ok` の上書きは
`config.toml` に置いてください（再 calibrate でユーザーの好みが上
書きされないように）。

```toml
[meta]
created_at         = "2026-04-30T09:00:00Z"
codebase_files     = 142
calibrated_at_sha  = "a0a6d1a7f3…"   # calibration 時点の HEAD sha（フル）
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
p90 = 67.0          # Hotspot 🔥 フラグ境界（上位 10%、固定）
p95 = 145.0
```

`calibrated_at_sha` は任意項目で、`heal calibrate --force` が記録
します。`/heal-config` スキルはこれと `codebase_files`、現在の
`.heal/findings/latest.json` および `.heal/findings/fixed.json` を
合わせて calibration ドリフトを検知し、`heal calibrate --force`
を提案するかを判定します。

`calibration.toml` が既にある状態でフラグ無しの `heal calibrate`
を実行しても、ファイルがあることを報告するだけで書き換えは行いま
せん。実際に再スキャン・上書きするには `--force` を付けます。

## `[policy.drain]`

drain ポリシーは、`heal-code-patch` skill が drain しなければなら
ない `(Severity, hotspot)` の組（T0）と、余裕があれば drain する組
（T1）を決めます。どちらにも該当しない非 Ok の Finding は Advisory
に落ち、`heal check --all` でのみ表示されます。

```toml
[policy.drain]
must   = ["critical:hotspot"]            # T0 — drain to zero
should = ["critical", "high:hotspot"]    # T1 — 余裕があれば
```

DSL 文法:

- `<severity>` — その severity に該当（hotspot 不問）。
- `<severity>:hotspot` — severity に該当 **かつ** `hotspot = true`。

Severity トークンは小文字: `critical / high / medium / ok`。未知の
トークンや余分な `:` セグメントは config 読み込み時に拒否されます
（暗黙のドリフトなし）。

厳格チーム: `must = ["critical:hotspot", "critical", "high:hotspot"]`
で T0 がデフォルトの T1 範囲も拾うようにできます。緩めチーム:
`should = []` で T0 を厳格に保ちつつ T1 を Advisory に折り畳めます。

ユーザー定義の名前付きポリシーは `[policy.rules.<name>]` 配下に置
きます。これは v0.4 のメトリクスドリフトアクション用に予約されて
おり、現状は parse のみ。スキーマはリリースをまたいで保持されます。

```toml
[policy.rules.high_complexity_new_function]
action = "report-only"
```

## 厳格設計

すべてのセクションで `deny_unknown_fields` が有効です。キーをタイ
ポすると、暗黙にドロップされるのではなく、起動時にパースエラーにな
ります。

```toml
[metrics]
typo_n = 5     # ✘ 不明なフィールド — heal はこの行でエラーになります
```

パースエラーには違反したキーのファイルパスと行番号が含まれます。こ
のトレードオフは意図的です — 暗黙のドロップは設定ドリフトが本番に
向かう定番経路です。
