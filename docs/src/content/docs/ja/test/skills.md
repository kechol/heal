---
title: Test · スキル
description: "[features.test] 向け同梱スキル 3 種 — /heal-test-reporter-setup、/heal-test-review、/heal-test-patch。Claude Code / OpenAI Codex 対応。"
---

オプトインの **Test** ファミリはスキルを 3 種同梱しています。`heal init` 時に検出した各エージェントターゲットへ Code ファミリスキルと並んで展開されますが、test オブザーバが生む findings にしか作用しません。

インストール手順とドリフト認識付きの更新の仕組みは [Code › スキル](/heal/ja/code/skills/) を参照(共通の仕組みです)。

## `/heal-test-reporter-setup` — lcov を配線

ワンショットセットアップスキル。プロジェクトの言語スタックを判別し、`lcov.info` が heal のデフォルト `lcov_paths` のどこかに着地するよう、lcov リポータの設定と CI 統合を提案します。

| スタック | 提案 |
|---|---|
| Rust | `cargo llvm-cov --lcov --output-path lcov.info` |
| Python | `pytest --cov=src --cov-report=lcov`(`pytest-cov`) |
| JS / TS | `nyc --reporter=lcov mocha` / `vitest --coverage --coverage.reporter=lcov` |
| Go | `go test -coverprofile=coverage.out` + `gcov2lcov` |
| Scala | `scoverage` プラグイン + lcov リポータ |
| 混在 | スタック別の提案 + 単一の `lcov.info` を生成する CI ステップ |

コードベースに対しては読み取り専用で、コマンドや config 編集の **提案** にとどまります(実行はしません)。コマンドはユーザが実行し、heal が次の `heal status` で結果の `lcov.info` を読みます。

トリガーフレーズ: 「set up coverage reporting」、「configure lcov for heal」、「wire up coverage」、「/heal-test-reporter-setup」。

## `/heal-test-review` — 監査スキル

読み取り専用。`heal status --json` を読み、`[features.test]` スライスにフィルタリングし、次の 2 つを返します:

1. テストスイートの **アーキテクチャ的読解** — 支配的な軸は「unit テストがない」「テストがソースについていけていない」「重要パスがカバーされていない」「skip された flake が永続的な skip として固化した」のどれか?
2. **優先順位付きテスト修正 TODO リスト** — まず hotspot ファイルのカバレッジギャップ、次にドリフトしたテスト、最後に skip 比率の outlier。

ソースは編集しません。レビューを読んで「これも直してほしい」と思ったら、その場でエージェント(Claude Code / Codex)に伝えれば対応に移れます(「上位 3 件のテストを書いて」「`auth/` 配下の skip を再有効化して」など)。機械的な修正は `/heal-test-patch` を経由し、判断が要る項目(「実は本物の flake で skip のままが正しいのでは?」「このカバーされていないファイルはそもそもテストすべきか、削除すべきか?」)は自動適用されず、あなたの指示を待ちます。

### review と patch を分けている理由

**Patch** が引き受けるのは機械的な修正です — 明らかに未カバーな分岐に unit テストを書く、ドリフトしたテストを対象関数に合わせ直す、解決済みの理由(「issue #123 待ち」)が残っている skip を再有効化する。**Review** はそれに加えて、**人間の判断が必要な項目** を拾い上げます — assertion が弱く見えるもの、本物の flake を隠している疑いがある skip、そもそもこのレイヤでテストすべきでないかもしれないファイル。両者を 1 つのオート実行に混ぜると、本物の flake を覆い隠すか、簡単に書けるテストを放置するか、どちらかになります。

トリガーフレーズ: 「review the test health」、「where should we add tests」、「which tests should we unskip」、「/heal-test-review」。

## `/heal-test-patch` — 書き込みスキル

`.heal/findings/latest.json` のテストスライスを Severity 順に 1 件ずつ消化します。**1 修正 1 コミット**。

**事前チェック**(失敗すると起動拒否):

- クリーンな worktree。
- キャッシュ存在(欠けていれば `heal status --json` で埋める)。
- `[features.test]` 有効(`.heal/config.toml`)。
- `coverage_pct` の finding がスコープ内なら、`lcov.info` が `lcov_paths` のどこかに存在すること(無ければ `/heal-test-reporter-setup` に誘導)。

**メトリクス別の手筋**:

| メトリクス | デフォルトの手 |
|---|---|
| `coverage_pct` | 未カバーの hot path に unit テストを書く / 拡張する。コミットごとに coverage リポータを再実行して `lcov.info` を更新。 |
| `skip_ratio` | 理由が成立しなくなった skip を再有効化。skip マーカーを削除し、テストを実行、失敗があればその場で修正。 |
| `change_coupling.drift` | ドリフトしたテストとソースを一緒に表面化し、テストをソースの現在の形に合わせる。 |

**Refusal**(スキル本体に encoded、プロンプトしても上書きされません):

- **assertion-weakening** — 古いテストを通すために `assert.equal(x, 5)` を `assert.ok(x)` に書き換えない。アサーションが間違っているなら、理由をコミットメッセージで明記してテストを削除する。
- **skip-the-flake** — flaky なテストを消すために skip マーカーを追加しない。
- **scaffold-without-running** — すべてのコミットでテストスイートを実行する。「正しそう」だが実行されていないテストはランディングしない。

**制約**: 1 finding = 1 commit、Conventional Commit subject + `Refs: F#<finding_id>` trailer、push / amend / `--no-verify` はしない。Code または Docs ファミリのメトリクスに属する findings はスキップ。

トリガーフレーズ: 「fix the test findings」、「drain the test cache」、「add tests heal flagged」、「/heal-test-patch」。
