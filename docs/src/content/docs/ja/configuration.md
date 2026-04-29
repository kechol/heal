---
title: 設定
description: .heal/config.toml の読み方と編集方法 — 全セクションを解説、現実的な例つき。
---

`heal init` は `.heal/config.toml` を書き出します。すべての heal の
設定には妥当なデフォルトがあり、初期インストールのままで動作します。
プロジェクト固有の値で上書きしたいときだけファイルを編集してくださ
い。

## ファイルの場所

```
<your-repo>/.heal/config.toml
```

設定はリポジトリごとです。グローバル設定はありません。heal は呼び
出しごとにファイルを読み直すため、再起動が必要なデーモンもありませ
ん。

## おすすめのスタート地点

ほとんどのプロジェクトでは、次の設定が良い出発点になります。情報量
の多いメトリクスを有効にし、生成ファイルを除外し、参照ウィンドウを
90 日に設定しています。

```toml
[project]
# Claude に英語で応答してほしい場合（デフォルト）はこの行を削除します。
response_language = "Japanese"

[git]
since_days = 90
exclude_paths = ["dist/", "vendor/", "node_modules/", ".cache/"]

[metrics]
top_n = 5

[metrics.cognitive]
enabled = true

[metrics.churn]
enabled = true

[metrics.hotspot]
enabled = true
weight_churn = 1.0
weight_complexity = 1.5

[policy.complexity_spike]
cooldown_hours = 24
```

これで Hotspot ランキング、Churn、Cognitive 複雑度 — AI 支援開発の
日常で最も実用的な 3 つのシグナル — が手に入ります。Duplication と
Change Coupling はデフォルトでオフです。プロジェクトが大きくなり、
コピー＆ペーストのドリフトや意外なファイル間連動が気になり始めたタ
イミングで有効にしてください。

## デフォルト

| メトリクス       | デフォルト             |
| ---------------- | ---------------------- |
| LOC              | 常に有効（トグルなし） |
| Churn            | 有効                   |
| Cognitive        | 有効                   |
| CCN (complexity) | 無効                   |
| Duplication      | 無効                   |
| Change Coupling  | 無効                   |
| Hotspot          | 無効                   |

無効化されたメトリクスは完全にスキップされます。オブザーバーが走ら
ず、`heal status` にも現れません。有効にするには対応するセクション
で `enabled = true` を設定してください。

## `[project]`

プロジェクトレベルのメタデータ。

```toml
[project]
response_language = "Japanese"
```

- `response_language` — `heal check` に渡される自由記述の言語ヒント。
  Claude が解釈できる任意の値が使えます: `"Japanese"`、`"日本語"`、
  `"français"`、`"plain English"` など。任意項目で、未設定なら
  Claude のデフォルトが使われます。

## `[git]`

git の履歴を辿るメトリクス（Churn、Change Coupling、Hotspot）すべ
てが参照します。

```toml
[git]
since_days = 90
exclude_paths = ["dist/"]
```

- `since_days`（デフォルト `90`） — 参照ウィンドウ。これより古いコ
  ミットは無視されます。
- `exclude_paths`（デフォルト `[]`） — 無視する**パス部分文字列**の
  リスト。`"dist/"` は `packages/web/dist/foo.js` と
  `apps/api/dist/bar.js` の両方にマッチします。グロブパターンは未対
  応です。精度が必要なときは具体的な部分文字列を使ってください。

LOC オブザーバーはデフォルトでこのリストを継承します。他のオブザー
バーは常に尊重します。

## `[metrics]`

トップレベルのフィールドはオブザーバー間で共有されます。

```toml
[metrics]
top_n = 5
```

- `top_n`（デフォルト `5`） — すべての「worst-N」表示のデフォルトサ
  イズ。各オブザーバー個別で `top_n` を上書きできます。

以下の各オブザーバーサブセクションには、共通して 2 つのパターンが
あります。

- `enabled` — マスター切り替え（LOC は常時有効でトグルなし）。
- `top_n`（任意） — そのオブザーバーのランキング用にグローバルデフォ
  ルトを上書き。

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

複雑度オブザーバーの設定です。`ccn`（Cyclomatic）はデフォルト無効、
`cognitive`（Sonar スタイル）はデフォルト有効です。

```toml
[metrics.ccn]
enabled = true
warn_delta_pct = 30

[metrics.cognitive]
enabled = true
```

- `ccn.warn_delta_pct`（デフォルト `30`） — `max_ccn` の変化率（パー
  セント）。これを超えると SessionStart の "complexity spike" ルー
  ルが発火します。

### `[metrics.duplication]`

```toml
[metrics.duplication]
enabled = true
min_tokens = 50
```

- `min_tokens`（デフォルト `50`） — 重複ブロックとみなす最小ウィン
  ドウ長。値を下げるとより多く・より短いブロックが拾われます。

### `[metrics.change_coupling]`

```toml
[metrics.change_coupling]
enabled = true
min_coupling = 3
```

- `min_coupling`（デフォルト `3`） — この回数より少なく共変したペア
  はランキング前にドロップされます。

### `[metrics.hotspot]`

```toml
[metrics.hotspot]
enabled = true
weight_churn = 1.0
weight_complexity = 1.0
```

- `weight_churn` と `weight_complexity`（どちらもデフォルト `1.0`）
  — 合成スコアは `(weight_complexity × ccn_sum) × (weight_churn ×
commits)` です。どちらかを `0.0` にすると、その側の合成のみ無効化
  されます（オブザーバー本体は無効化されません）。

## `[policy.<rule_id>]`

ルールごとに 1 ブロックです。ルールは SessionStart のナッジを駆動
します。しきい値を超えるとルールが発火し、次の Claude セッションの
冒頭に通知が表示されます。

```toml
[policy.complexity_spike]
action = "report-only"
cooldown_hours = 24
threshold = { ccn = 15, delta_pct = 20 }
```

- `action` — `report-only`、`notify`、`propose`、`execute` のいず
  れか。`report-only` は Claude のコンテキストに通知を出します。よ
  り高いアクションレベルでは自動応答が有効になります。
- `cooldown_hours`（デフォルト `24`） — 同じルールが 2 回発火する
  までの最小時間（時間単位）。
- `threshold` — ルール固有のしきい値。キーはルールに依存します。

heal がセッション開始時に評価する 5 つのルール:

| ルール ID                      | 発火条件                                       |
| ------------------------------ | ---------------------------------------------- |
| `hotspot.new_top`              | トップホットスポットファイルが入れ替わった     |
| `complexity.new_top_ccn`       | トップ CCN 関数が入れ替わった                  |
| `complexity.new_top_cognitive` | トップ Cognitive 関数が入れ替わった            |
| `complexity.spike`             | `max_ccn` が `warn_delta_pct` 以上ジャンプした |
| `duplication.growth`           | 重複トークン数が増加した                       |

ルールを明示的に宣言する必要はありません。エントリのないルールはデ
フォルト（`action = "report-only"`、`cooldown_hours = 24`）が適用さ
れます。

## 厳格設計

すべてのセクションで `deny_unknown_fields` が有効です。キーをタイポ
すると、暗黙にドロップされるのではなく、起動時にパースエラーになり
ます。これは意図的なトレードオフです — 暗黙のドロップは本番に向か
う設定ドリフトの定番経路です。

```toml
[metrics]
typo_n = 5     # ✘ 不明なフィールド — heal はこの行でエラーになります
```

パースエラーには違反したキーのファイルパスと行番号が含まれます。
