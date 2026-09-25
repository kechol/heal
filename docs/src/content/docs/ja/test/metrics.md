---
title: Test · メトリクス
description: '[features.test] ファミリが加える、テストの質を見る 3 つのメトリクス（coverage_pct、skip_ratio、Test Hotspot）と、change_coupling.drift サブメトリクス。'
---

オプトインの **Test** ファミリは、常時オンの Code ファミリに、トップレベルのメトリクスを 3 つ加えます。`coverage_pct`、`skip_ratio`、Test Hotspot です。あわせて、`change_coupling` のサブメトリクスとして `change_coupling.drift` も加えます。中心になるシグナルは**行カバレッジ**です。外部のツールが生成した `lcov.info` から読み、Hotspot のスコアに反映するので、テストの届いていないよく変わる箇所がキューの上位に来ます。

設定項目は [Test › 設定](/heal/ja/test/configuration/) を、スキルは [Test › スキル](/heal/ja/test/skills/) を参照してください。

## 一覧

| メトリクス              | 単位                       | 何を見つけるか                                                                           |
| ----------------------- | -------------------------- | ---------------------------------------------------------------------------------------- |
| `coverage_pct`          | ソースファイルごと         | `lcov.info` から読んだ行カバレッジ。Finding を出すのは `< 100%` のファイルだけ           |
| `skip_ratio`            | テストファイルごと         | ファイル内のテストのうち、skip されているものの割合                                      |
| `test_hotspot`          | ソースファイルごと         | `commits × uncov_pct` の合成スコア。`coverage_pct` の Finding に `hotspot=true` を立てる |
| `change_coupling.drift` | ペアごと（サブメトリクス） | ソースは変わり続けているのに、一緒に変わっていないテスト                                 |

構造の面でも 1 つ加わります。すべての Finding に `is_test_file: bool` のフラグが付き、スキルがテスト側と本番側の Severity を分けて読めるようになります。

## `coverage_pct`

> _「テストから見えていない本番コードはどこか?」_

ソースファイルごとの行カバレッジです。`[features.test.coverage].lcov_paths` にある `lcov.info` のうち、存在するものをすべて読み、1 つにまとめます（多言語のモノレポでパッケージごとにファイルを並べれば、どれも集計に入ります）。Finding を出すのは、カバレッジが `< 100%` のファイルだけです。calibration では**反転した値**（`100 - coverage_pct`）を保存します。ほかのメトリクスと同じ「値が p95 に届いたら Critical」の段階をそのまま使えるようにするためです。フロアについては [Test › 設定](/heal/ja/test/configuration/#calibration) を参照してください。

カバレッジの出力には、観測の状態も入ります。`missing` は設定したレポートが見つからない、`read_error` は少なくとも 1 つを読めなかった、`partial` は LCOV に載っていない対象の本番ファイルがある（その一覧も出ます）、`complete` は欠けがない、という意味です。載っていないファイルは**未計測**として扱い、0% とはみなしません。リポータやパッケージの範囲の設定を見直すよう促し、Test の解消キューには入れません。ヒット数 0 の LCOV の記録は、計測済みの 0% として Finding になります。設定したテストのパス、生成されたファイル、除外したパスは、未計測の本番ファイルには数えません。

## `skip_ratio`

> _「skip されたテストが無視できない割合を占めるファイルはどれか?」_

テストファイルごとの、テストの総数に対する skip されたテストの割合です。heal は `[features.test].test_paths` に一致するファイルをたどり、言語ごとの skip の印を数えます。Rust の `#[ignore]`、Python の `@pytest.mark.skip` / `@unittest.skipIf`、JS / TS の `it.skip` / `xit` / `xdescribe`、Go の `t.Skip()`、ScalaTest の `ignore` / `pending` です。検出は構文を見て行うので、コメントや文字列の中の印を誤って数えることはありません。

## `change_coupling.drift`

> _「カバーしているソースについていけていないテストはどれか?」_

`[features.test]` がオンのときは、テストとソースのペアのうち、一緒に変わった回数がプロジェクトの中央値を**下回る**ものに注目します。テストがソースと一緒に動いていないペアです。こうしたペアを、`change_coupling.expected`（Advisory）から `change_coupling.drift`（Medium）に付け直します。「テストはあるが、ソースへの最近の変更はどれもテストなしで行われている」と読んでください。

ドキュメントとソースのペアが drift に格上げされることはありません。drift はテストの質のシグナルだからです。

## Test Hotspot — 変更が続くのにテストがない場所

Test Hotspot は、code の Hotspot の test ファミリ版です。ソースファイルを `commits × uncov_pct` でランク付けします。スコアが高いほど、そのファイルは変更が続いていて、**しかも**大部分がテストされていません。CCN が低い設定の読み込み処理でも、カバレッジが 0% で 30 回コミットされていれば、テストを書くべき本当の対象です。code の Hotspot では、これを見落とします。

このスコアに入るのは、LCOV に明記された本番ファイルだけです。載っていないものは未計測で、0% と明記されたものは 100% の不足として数えます。カバレッジが 100% のファイルは、スコアが 0 になって外れます。

有限の候補が 5 件以上あれば、Test Hotspot には p90 と Test のフロア（25）の両方が必要です。1〜4 件のときは、絶対値のフロアだけを使います。有限でないスコアにはフラグを立てません。Test の同じ Tier と Severity の中では、スコアの高いものから取り組みます。これは優先順位を決めるための目安で、確率でも効果の保証でもありません。

Test Hotspot 自体は常に `Severity::Ok` です。役目は、同じファイルの `coverage_pct` の Finding に `hotspot=true` を立てることです。そのため、解消の対象は「Critical かつ `hotspot=true`」のままで、範囲が test ファミリになるだけです。

## post-commit の通知:「uncovered hotspot」

```
heal: recorded · 3 critical, 7 high · heal status
         · 2 uncovered hotspot
```

この数は、High か Critical の `coverage_pct` の Finding のうち、`hotspot=true` も付いているものの件数です。次のテストをどこに書けばいいかを、いちばん短く伝える行です。`[features.test.coverage]` がオフのとき、または Hotspot のファイルに High / Critical の `coverage_pct` がないときは出ません。

## 解消のしかた

`/heal:tests` は、Finding をテストピラミッド（単体 / 結合 / E2E）の観点で整理し、承認された提案を 1 コミットずつ適用します。`coverage_pct` には足りない単体テストを書き、`skip_ratio` には skip の理由がなくなったテストを再び有効にし、`change_coupling.drift` にはずれたテストを揃え直します。アサーションを弱めたり、本当に不安定なテストをごまかしたりはしません。詳しい約束ごとは [Test › スキル](/heal/ja/test/skills/) を参照してください。
