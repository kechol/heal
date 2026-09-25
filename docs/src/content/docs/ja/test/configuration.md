---
title: Test · 設定
description: '[features.test] の有効にしかた、lcov.info の読ませかた、どのファイルをテストとして扱うかの調整。'
---

**Test** ファミリはオプトインで、既定では無効です。`cargo llvm-cov`、`pytest --cov`、`nyc`、`scoverage` のどれかが出力した `lcov.info` がある（または用意するつもりがある）なら、有効にしてください。heal 自身がテストを実行することはありません。テストスイートを実際に動かさないと分からないもの（不安定なテスト、ミューテーションスコア、実行時間の推移）は扱いません。

各メトリクスが何を見つけるかは [Test › メトリクス](/heal/ja/test/metrics/) を、スキルについては [Test › スキル](/heal/ja/test/skills/) を参照してください。

## すぐに有効にする

```toml
[features.test]
enabled = true

[features.test.coverage]
enabled = true
```

既定値は、Rust / TypeScript / JavaScript / Python / Go / Scala のテストの慣習に合わせてあります。`lcov.info` も、よく使われる 4 か所を探します。ほとんどのプロジェクトでは、何も上書きする必要はありません。

`lcov.info` がまだないなら、セットアップ用スキルの tests の手順を実行してください。使っている技術スタックを調べてカバレッジのリポータを設定します。各手順の前には確認を求めます。Claude Code で次を実行します。

```
/heal:setup tests
```

詳しくは [Test › スキル](/heal/ja/test/skills/) を参照してください。

## `[features.test]`

```toml
[features.test]
enabled    = false                # ファミリ全体のスイッチ
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

- `enabled`（既定 `false`）— ファミリ全体のスイッチ。false のあいだ、テストのオブザーバはどれも何もしません。
- `test_paths`（既定は上の言語ごとの慣習）— どのソースファイルがテストかを示す、gitignore 形式のグロブ。`skip_ratio` のオブザーバはこのファイルをたどります。主なファイルがこれに一致する Finding には、`is_test_file = true` も付きます。

`test_paths` が空のときは、同じ慣習をカバーする組み込みの判定を使います。

グロブは `.gitignore` と同じように、置き場所が固定されます。既定の `tests/**` は、プロジェクトのルートにある `tests/` ディレクトリにしか一致しません。`crates/<name>/tests/` のようにテストのディレクトリが入れ子になった workspace では、`**/tests/**` を使ってください。`test_paths` を自分で設定すると、[Semantic (Jev)](/heal/ja/semantic/) のタスクもそのグロブだけを使います。既定のままなら、組み込みの判定も加わり、`test/` という名前のディレクトリはすべてテストとして扱われます。

### `is_test_file` フラグ

`[features.test]` を有効にすると、すべての Finding に `is_test_file: bool` のフラグが付きます。スキルはこのフラグで絞り込み、テスト側と本番側の Severity を分けて読みます。`/heal:tests` はテストの Finding に、`/heal:refactor` は本番コードの Finding に集中します。

フラグが false のときは JSON の出力から省くので、test ファミリを有効にしていないプロジェクトの `latest.json` は、以前とバイト単位で同じです。

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

- `enabled`（既定 `false`）— カバレッジの部分だけのスイッチ。リポータの設定はまだだが、`is_test_file` のタグ付けと `skip_ratio` は使いたいときは、`[features.test]` をオンにして `[features.test.coverage]` をオフにしておきます。
- `lcov_paths` — 順に探す、プロジェクトからの相対パス。**存在するファイルはすべて読んでマージする**ので、多言語のモノレポでもパッケージごとの `lcov.info` を並べれば、どれも集計に入ります。2 つのファイルが同じソースファイルを扱っているときは、カウンタの大きいほうを取ります（1 つの lcov ファイルの中で記録が重複したときと同じ扱いです）。見つからないファイルは黙って飛ばし、起動時の警告も出しません。存在するのに読めないファイルは、標準エラーに警告を出します。リポータがパッケージのルートからの相対パスで書いた `SF:` のパス（vitest、jest、scoverage）は、その lcov ファイルが置かれたディレクトリを基準に解決します。そのため、マージ用のスクリプトがなくてもパッケージごとのファイルがそのまま使えます。
- `post_commit_refresh`（既定は未設定）— post-commit フックが、コミットのたびにリポータを実行し直すためにバックグラウンドで起動するシェルコマンド（任意）。プロセスは切り離され、出力も捨てるので、コミットの流れを待たせません。`/heal:setup tests` が提案するのと同じコマンド（`cargo llvm-cov --workspace --lcov --output-path lcov.info --locked --ignore-run-fail`、`pytest --cov=...` など）を設定すれば、次の `heal status` が新しい `lcov.info` を読めます。`[features.test]` か `[features.test.coverage]` がオフのときは、黙って実行しません。

heal が読むのは、CI や手元のリポータが出力したものです。既定で探す場所は、次のリポータに対応しています。

| リポータ                         | 書き出すパス                                                  |
| -------------------------------- | ------------------------------------------------------------- |
| `cargo llvm-cov --lcov`          | `target/llvm-cov/lcov.info`                                   |
| `pytest --cov --cov-report=lcov` | `coverage/lcov.info`                                          |
| `nyc --reporter=lcov`            | `coverage/lcov-report/lcov.info`                              |
| `scoverage`（Scala）             | 設定による。必要なら `lcov.info` へのシンボリックリンクを置く |

lcov の読み込みは寛容に作っています。知らない種類の記録があっても止まらず、リポータが集計のフィールドを省いた場合も行ごとの記録から合計を求めるので、たいていのリポータの方言はそのまま使えます。

## Calibration

test ファミリを有効にした状態で `heal calibrate --force` を実行すると、`.heal/calibration.toml` に 2 つの節が加わります。

```toml
[calibration.coverage_pct]
# heal は値を反転して（100 - coverage_pct）保存する。ほかのメトリクスと
# 同じ「value >= p95 なら Critical」の段階をそのまま使えるようにするため。
# 最も悪いものが Critical になる点は変わらない。
p50 = 30.0     # カバレッジ 70%
p75 = 50.0     # カバレッジ 50%
p90 = 70.0     # カバレッジ 30%
p95 = 85.0     # カバレッジ 15%
floor_critical = 95.0   # カバレッジ 5% 以下 → パーセンタイルにかかわらず Critical
floor_ok       = 25.0   # カバレッジ 75% 超 → パーセンタイルにかかわらず Ok

[calibration.skip_ratio]
p50 = 0.0
p75 = 1.0
p90 = 5.0
p95 = 10.0
floor_critical = 20.0   # skip が 20% 超 → Critical
floor_ok       = 0.5    # skip が 0.5% 未満 → Ok
```

これは、`heal calibrate --force` を実行するまで heal が使う、文献に基づく代わりの値です。フロアは calibration をやり直しても消えないように、ここではなく `config.toml` に書きます。

```toml
[metrics.coverage_pct]
floor_critical = 90.0   # 「カバレッジ 10% 以下なら Critical」に厳しくする

[metrics.skip_ratio]
floor_ok = 0.0          # skip されたテストが 1 つでもあれば表示する
```

（`coverage_pct` の上書きは、反転した値に対して効きます。`floor_critical = 90.0` は「行カバレッジが 10% 以下」という意味で、「90% 以下」ではありません。）

## post-commit の通知

`[features.test.coverage]` がオンのとき、post-commit フックの通知には、字下げした 2 行目が加わります。

```
heal: recorded · 3 critical, 7 high · heal status
         · 2 uncovered hotspot
```

この数は、High か Critical の `coverage_pct` の Finding のうち、`hotspot=true` も付いているものの件数です。カバレッジの機能がオフのときは、この行は出ません。

## 厳密な設計

ほかの節と同じく、`[features.test]` と `[features.test.coverage]` も、定義されていないキーがあるとエラーにします。

```toml
[features.test]
test_path = ["tests/**"]   # ✘ 知らない項目 — heal はここでエラーを出す
                            #   （正しくは複数形の `test_paths`）
```
