---
title: Test · 設定
description: "[features.test] の有効化、lcov.info の配線、テストとして扱うファイルの指定方法。"
---

**Test** ファミリはオプトインです。デフォルトでオフ。`cargo llvm-cov`、`pytest --cov`、`nyc`、`scoverage` などのリポータが生成した `lcov.info` がある(または用意できる)ときに有効化してください。heal はテストを実行しません。テストスイートを実際に走らせて初めて分かること(flakiness、mutation score、ランタイム傾向など)はスコープ外です。

各メトリクスが捕まえる内容は [Test › メトリクス](/heal/ja/test/metrics/)、同梱スキルは [Test › スキル](/heal/ja/test/skills/) を参照。

## 有効化の手順

```toml
[features.test]
enabled = true

[features.test.coverage]
enabled = true
```

デフォルトが Rust / TypeScript / JavaScript / Python / Go / Scala のテスト規約と 4 つの慣習的な `lcov.info` パスをカバーするので、ほとんどのプロジェクトは何も上書きする必要はありません。

`lcov.info` がまだない場合は、同梱のセットアップスキルを実行してください。スタックを判別してリポータの配線を提案します。

```sh
claude /heal-test-reporter-setup
```

スキルの完全な仕様は [Test › スキル](/heal/ja/test/skills/#heal-test-reporter-setup--lcov-を配線) を参照。

## `[features.test]`

```toml
[features.test]
enabled    = false                # マスタースイッチ
test_paths = [
  "tests/**",
  "**/*_test.rs",
  "**/*.test.ts", "**/*.test.tsx", "**/*.test.js", "**/*.test.jsx",
  "**/*.spec.ts", "**/*.spec.tsx", "**/*.spec.js", "**/*.spec.jsx",
  "**/__tests__/**",
  "**/*_test.go",
  "**/test_*.py", "**/*_test.py",
  "**/*Test.scala", "**/*Spec.scala",
]
```

- `enabled`(デフォルト `false`) — マスタースイッチ。false の間は全テストオブザーバが no-op になります。
- `test_paths`(デフォルト: 上記の言語規約) — どのソースファイルがテストかを示す gitignore 構文のグロブ。`skip_ratio` がこのリストに沿ってファイルを歩き、各 Finding のプライマリファイルがマッチすれば `is_test_file = true` のタグも付きます。

`test_paths` が空のときは、同じ言語規約をハードコードしたフォールバックヒューリスティックが適用されます。

### `is_test_file` フラグ

`[features.test]` を有効にすると、各 Finding に `is_test_file: bool` フラグが追加されます。スキルはこのフラグでフィルタリングして、テスト側と本番側の severity を独立に読みます。`/heal-test-review` はテスト findings に集中し、`/heal-code-review` は本番 findings に集中します。

フラグは false のとき JSON 出力から省略されるので、test ファミリを有効化していないプロジェクトは従来とバイト同等の `latest.json` を保てます。

## `[features.test.coverage]`

```toml
[features.test.coverage]
enabled    = false
lcov_paths = [
  "lcov.info",
  "coverage/lcov.info",
  "target/llvm-cov/lcov.info",
  "coverage/lcov-report/lcov.info",
]
```

- `enabled`(デフォルト `false`) — サブ機能スイッチ。`[features.test]` はオン、`[features.test.coverage]` はオフのまま、というのも有効です(`is_test_file` タグ付けと `skip_ratio` だけ使い、リポータ配線は後で行う、というケース)。
- `lcov_paths` — プロジェクト相対のパスを順に探索します。**最初に存在するファイルが勝ち**、残りは無視されます。欠けているファイルは silent で、起動時に警告は出ません。

heal は CI / ローカルリポータが書き出したものを読みます。デフォルトの探索順がカバーするのは:

| リポータ | 書き出すパス |
|---|---|
| `cargo llvm-cov --lcov` | `target/llvm-cov/lcov.info` |
| `pytest --cov --cov-report=lcov` | `coverage/lcov.info` |
| `nyc --reporter=lcov` | `coverage/lcov-report/lcov.info` |
| `scoverage`(Scala) | プラグイン依存。必要なら `lcov.info` にシンボリックリンク |

lcov リーダーは寛容で、未知のレコードタイプを許容したり、リポータが summary を省略したときに per-line レコードから合計を復元したりします。多くのリポータ方言がそのまま動きます。

## Calibration(Severity 基準の調整)

`heal calibrate --force` を test ファミリ有効状態で走らせると、`.heal/calibration.toml` に新しい 2 セクションが書かれます:

```toml
[calibration.coverage_pct]
# heal は **反転値**(100 - coverage_pct)を保存するので、他のメトリクスと
# 同じ「value が p95 に達したら Critical」のカスケードがそのまま使え、
# 「最悪が Critical」を意味し続けます。
p50 = 30.0     # カバレッジ 70%
p75 = 50.0     # カバレッジ 50%
p90 = 70.0     # カバレッジ 30%
p95 = 85.0     # カバレッジ 15%
floor_critical = 95.0   # ≤ 5% カバレッジ → percentile に関係なく Critical
floor_ok       = 25.0   # > 75% カバレッジ → percentile に関係なく Ok

[calibration.skip_ratio]
p50 = 0.0
p75 = 1.0
p90 = 5.0
p95 = 10.0
floor_critical = 20.0   # > 20% skip → Critical
floor_ok       = 0.5    # < 0.5% skip → Ok
```

ここに書いた値は、`heal calibrate --force` を実行するまでに使われる文献由来のフォールバックです。フロアは `config.toml` 側に置いてください(再 calibration を生き延びるように):

```toml
[metrics.coverage_pct]
floor_critical = 90.0   # 「≤ 10% カバレッジ → Critical」へ締める

[metrics.skip_ratio]
floor_ok = 0.0          # skip されたテストはすべて表示
```

(`coverage_pct` の上書きは反転形式に対して適用されます。`floor_critical = 90.0` は「≤ 10% 行カバレッジ」の意味で、「≤ 90%」ではありません。)

## post-commit ナッジ

`[features.test.coverage]` を有効にすると、post-commit フックがナッジにインデント付き 2 行目を追加します:

```
heal: recorded · 3 critical, 7 high · heal status
         · 2 uncovered hotspot
```

カウントは `coverage_pct` finding のうち Severity が High または Critical で、かつ `hotspot=true` のものです。カバレッジ機能がオフのときはこの行は出ません。

## 厳密設計

`[features.test]` と `[features.test.coverage]` も、他のセクションと同じく未知のキーを拒否します:

```toml
[features.test]
test_path = ["tests/**"]   # ✘ unknown — heal はここでエラー
                            #   (正しくは複数形 `test_paths`)
```
