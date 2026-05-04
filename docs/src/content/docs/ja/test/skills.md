---
title: Test · スキル
description: "[features.test] 向け同梱 Claude Code スキル 3 種 — /heal-test-reporter-setup、/heal-test-review、/heal-test-patch。"
---

オプトインの **Test** ファミリは Claude スキルを 3 種同梱しています。`heal skills install` / `heal init` で Code ファミリスキルと並んで展開されますが、test オブザーバが生む findings にしか作用しません。

インストール手順とドリフト認識付きの更新の仕組みは [Code › スキル](/heal/ja/code/skills/) を参照(共通の仕組みです)。

## `/heal-test-reporter-setup` — lcov を配線

ワンショットセットアップスキルです。プロジェクトの言語スタック(Rust / Python / JS-TS / Go / Scala / 混在)を判別し、`lcov.info` が heal のデフォルト `lcov_paths` のどれかに着地するように、lcov リポータの設定と CI 統合を提案します。

| スタック | 提案 |
|---|---|
| Rust | `cargo install cargo-llvm-cov` + `cargo llvm-cov --lcov --output-path lcov.info` |
| Python | `pytest --cov=src --cov-report=lcov`(`pytest-cov` 経由) |
| JS / TS | `nyc --reporter=lcov mocha` / `vitest --coverage --coverage.reporter=lcov` |
| Go | `go test -coverprofile=coverage.out` + `gcov2lcov` |
| Scala | `scoverage` プラグイン + lcov リポータ |
| 混在 | スタック別の提案 + 単一の `lcov.info` を生成する CI ステップ |

コードベースに対しては読み取り専用で、コマンドや config 編集の **提案** にとどまります(実行はしません)。コマンドはユーザが実行し、heal が次の `heal status` で結果の `lcov.info` を読みます。

トリガーフレーズ: 「set up coverage reporting」、「configure lcov for heal」、「wire up coverage」、「/heal-test-reporter-setup」。

## `/heal-test-review` — 監査スキル

読み取り専用です。`heal status --json` を読み、`Finding.metric` と `Finding.is_test_file` で `[features.test]` スライスにフィルタリングし、次の 2 つを返します:

1. テストスイートの **アーキテクチャ的読解**。支配的な軸は「unit テストがない」「テストがソースについていけていない」「重要パスがカバーされていない」「skip された flake が永続的な skip として固化した」のどれか?
2. **優先順位付きテスト修正 TODO リスト** — まず hotspot ファイルのカバレッジギャップ、次にドリフトしたテスト、最後に skip 比率の outlier。

`/heal-test-review` は提案のみで、ソースは編集しません。書き込み側のカウンターパートは `/heal-test-patch` です。

トリガーフレーズ: 「review the test health」、「where should we add tests」、「which tests should we unskip」、「/heal-test-review」。

## `/heal-test-patch` — 書き込みスキル

`.heal/findings/latest.json` のテストスライスを Severity 順に 1 件ずつ drain します。**1 修正 1 コミット** です。

事前チェック(失敗すると起動拒否):

1. クリーンな worktree。
2. キャッシュ存在(欠けていれば `heal status --json` を走らせて埋めます)。
3. `[features.test]` 有効(`.heal/config.toml` で)。
4. `coverage_pct` の finding がスコープ内なら、`lcov.info` が `lcov_paths` のどこかに存在すること。無ければ `/heal-test-reporter-setup` に誘導します。

メトリクス別の drain パターン:

| メトリクス | デフォルトの手 |
|---|---|
| `coverage_pct` | カバレッジ未達の hot path に対する unit test を書く / 拡張する。コミットごとに coverage リポータを再実行して `lcov.info` を更新する。 |
| `skip_ratio` | 理由が成立しなくなった skip テストを再有効化する。skip マーカーを削除し、テストを実行し、失敗があればその場で修正する。 |
| `change_coupling.drift` | ドリフトしたテストとソースのペアを一緒に表面化し、テストをソースの現在の形に合わせる。 |

スキル本体に encoded された refusal — プロンプトしても上書きされません:

- **assertion-weakening** — 古いテストを通すために `assert.equal(x, 5)` を `assert.ok(x)` に書き換えることはしません。アサーションが間違っているなら、コミットメッセージで理由を明記してテストを削除します。silent な緩和はしません。
- **skip-the-flake** — flaky なテストを消すために skip マーカーを追加することはしません。flakiness はそれ自体が問題で、別の修正に属します。
- **scaffold-without-running** — すべてのコミットでテストスイート(または言語別の相当物)を実行します。「正しそう」だが実行されていないテストはランディングしません。

スキルが強制する制約:

- 1 finding = 1 commit。
- Conventional Commit の subject + body + `Refs: F#<finding_id>` トレーラ。
- push しない、amend しない、`--no-verify` しない。

`/heal-test-patch` は Code または Docs ファミリのメトリクスに属する findings をスキップします。

トリガーフレーズ: 「fix the test findings」、「drain the test cache」、「add tests heal flagged」、「/heal-test-patch」。
