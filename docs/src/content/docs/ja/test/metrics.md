---
title: Test · メトリクス
description: '[features.test] ファミリが追加するテスト品質メトリクス 3 つ — coverage_pct、skip_ratio、Test Hotspot — と change_coupling.drift サブメトリクス。'
---

オプトインの **Test** ファミリは、常時オンの Code ファミリの上にトップレベルのメトリクスを 3 つ追加します — `coverage_pct`・`skip_ratio`・Test Hotspot です。これに加えて `change_coupling` に `change_coupling.drift` というサブメトリクスが乗ります。中心的なシグナルは **行カバレッジ** で、外部生成された `lcov.info` を読み取り、Hotspot のスコアに反映させてカバレッジ未達の hot path がキューの上位に浮かぶようにします。

設定の調整値は [Test › 設定](/heal/ja/test/configuration/)、同梱スキルは [Test › スキル](/heal/ja/test/skills/) を参照。

## 一覧

| メトリクス              | レイヤ                   | 何を捕まえるか                                                                        |
| ----------------------- | ------------------------ | ------------------------------------------------------------------------------------- |
| `coverage_pct`          | ソースファイル単位       | `lcov.info` から読んだ行カバレッジ。Finding は `< 100%` のファイルにのみ発行          |
| `skip_ratio`            | テストファイル単位       | ファイル内の skip テスト数 / 総テスト数(%)                                            |
| `test_hotspot`          | ソースファイル単位       | `commits × uncov_pct` の合成スコア。`coverage_pct` Finding に `hotspot=true` を立てる |
| `change_coupling.drift` | ペア単位(サブメトリクス) | ソースだけが先に変わっていて、テストがついていけていないペア                          |

加えて構造的な追加: 各 Finding に `is_test_file: bool` フラグが付き、スキルがテスト側と本番側の Severity を独立に読めるようになります。

## `coverage_pct`

> _「テストスイートから見えていない本番コードはどこか?」_

`[features.test.coverage].lcov_paths` に存在するすべての `lcov.info` を parse・マージした、ソースファイル単位の行カバレッジ率です(多言語モノレポならパッケージごとのファイルを列挙すればどれも集計に入ります)。Finding は `< 100%` のファイルにのみ発行されます。Calibration は **反転値**(`100 - coverage_pct`)を保存するので、他のメトリクスと同じ「value が p95 に達したら Critical」のカスケードがそのまま使えます — フロアの調整は [Test › 設定](/heal/ja/test/configuration/#calibrationseverity-基準の調整) を参照。

## `skip_ratio`

> _「skip テストの比率が無視できないファイルはどれか?」_

テストファイル単位の skip 比率(skip 数 / 総テスト数、パーセンテージ)です。`[features.test].test_paths` でマッチしたファイルを歩き、言語別の skip マーカーをカウントします — Rust の `#[ignore]`、Python の `@pytest.mark.skip` / `@unittest.skipIf`、JS / TS の `it.skip` / `xit` / `xdescribe`、Go の `t.Skip()`、ScalaTest の `ignore` / `pending`。検出は構造的なので、コメントや文字列リテラル中のマーカーで false positive が出ることはありません。

## `change_coupling.drift`

> _「カバーするソースについていけていないテストはどれか?」_

`[features.test]` を有効にすると、テスト ↔ ソースのペアの合算 co-change カウントがプロジェクトの中央値を **下回る**(テストがソースの動きについていけていない)とき、`change_coupling.expected`(Advisory)から `change_coupling.drift`(Medium)に再タグ付けされます。「テストは存在するが、ソースの最近の変更がすべてテストなしで起きている」という意味で読んでください。

ドキュメント ↔ ソースのペアは drift に昇格しません。drift はテスト品質シグナルだからです。

## Test Hotspot — 変更があるのにテストがない箇所はどこか

Test Hotspot は code Hotspot の test ファミリ版です。src ファイルを `commits × uncov_pct` でランクします。スコアが高い = そのファイルは編集が続いている **かつ** 大部分がテストされていない、という意味です。30 commits ある低 CCN の config-loader でカバレッジ 0% なら本物のテスト対象ですが、code Hotspot は CCN が低いせいで取りこぼします。

lcov に出てこないが git 履歴では触られているファイルは 100% gap(= 未テスト)として扱います。100% カバレッジのファイルはスコア 0 で落ちます。

Test Hotspot 自体は常に `Severity::Ok` です。スコアの仕事は同じファイルの `coverage_pct` Finding に `hotspot=true` を立てることです。解消対象は「Critical AND `hotspot=true`」のままで、test ファミリ単位にスコープされます。

## post-commit ナッジ:「uncovered hotspot」

```
heal: recorded · 3 critical, 7 high · heal status
         · 2 uncovered hotspot
```

カウントは `coverage_pct` Finding のうち Severity が High または Critical で、かつ `hotspot=true` のものです。「次のテストはここに書くべき」の最短リマインダ。`[features.test.coverage]` がオフのとき、または該当する Finding がないときはこの行は出ません。

## 解消パターン

`/heal-test-review` はテストピラミッドのレンズ(unit / integration / e2e)で findings をフレーム化します。`/heal-test-patch` は 1 コミット 1 件で消化していきます — `coverage_pct` には未カバーの hot path に unit テストを書く、`skip_ratio` には理由の成立しなくなった skip を再有効化、`change_coupling.drift` にはドリフトしたテストとソースを揃え直す。assertion を弱める / 本物の flake を覆い隠すといった refusal はスキル本体に encoded されています。詳しい契約は [Test › スキル](/heal/ja/test/skills/) を参照。
