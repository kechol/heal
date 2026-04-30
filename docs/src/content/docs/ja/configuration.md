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

| メトリクス       | デフォルト                       |
| ---------------- | -------------------------------- |
| LOC              | 常に有効（トグルなし）           |
| Churn            | 有効                             |
| Complexity (CCN) | 有効                             |
| Cognitive        | 有効                             |
| Duplication      | 有効                             |
| Change Coupling  | 有効（symmetric を含む）         |
| Hotspot          | 有効                             |
| LCOM             | 有効（`tree-sitter-approx`）     |

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
warn_delta_pct = 30
floor_critical = 25     # McCabe "untestable in practice"

[metrics.cognitive]
enabled        = true
floor_critical = 50     # SonarQube Critical のベースライン
```

- `ccn.warn_delta_pct`（デフォルト `30`） — `heal status` の差分ブ
  ロックで表示される `max_ccn` の変化率（パーセント）。
- `floor_critical` — Severity の絶対フロア。`≥ floor` のものはパー
  センタイル位置に関係なく Critical に分類されるため、コードベース
  全体が悪い場合でもワーストケースを表面化できます。

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

## `.heal/calibration.toml`

`heal init` が生成し、`heal calibrate` で更新します。手編集は推奨
されません — `floor_critical` の上書きは `config.toml` に置いてく
ださい（再 calibrate でユーザーの好みが上書きされないように）。

```toml
[meta]
created_at      = "2026-04-30T09:00:00Z"
codebase_files  = 142
strategy        = "percentile"

[calibration.ccn]
p50 = 4.2
p75 = 8.1
p90 = 14.3
p95 = 21.7
floor_critical = 25.0

[calibration.hotspot]
p50 = 5.0
p75 = 18.0
p90 = 67.0          # Hotspot 🔥 フラグ境界（上位 10%、固定）
p95 = 145.0
```

`heal calibrate --check` で自動検知トリガー（90 日経過、ファイル数
±20%、30 日連続で Critical が 0）を再 calibrate せずに評価できます。
post-commit ナッジは同じヒントをインラインで表示します。

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
