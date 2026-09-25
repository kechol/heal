---
title: 機能
description: heal は Code・Test・Docs の 3 つのファミリでコードヘルスを観測します。Code は常時オン、Test と Docs はオプトインです。
---

heal のオブザーバは 3 つのファミリに分かれています。**Code** は `heal init` でどのプロジェクトにも入る常時オンのファミリ。**Test** と **Docs** はオプトインで、`.heal/config.toml` で有効化したときにコードのメトリクスと並んで表示されます。各ファミリは独自のメトリクスと設定セクションに加えて、修正を提案し、承認されたものを適用する専用の Claude スキルを持ちます。

## Code(常時オン)

> _「コードベースのどこが変更しにくいか?」_

デフォルトのオブザーバファミリです。LOC・CCN・Cognitive Complexity・Churn・Change Coupling・Duplication・LCOM の 7 メトリクスを、コードベース自身の分布に合わせてcalibrate し、`heal status` で表示します。`🔥` の Hotspot 装飾は、複雑かつ頻繁に編集されるファイル(リグレッションが集中しがちな場所)を強調します。

| ページ                                        | こんなときに読む                                                            |
| --------------------------------------------- | --------------------------------------------------------------------------- |
| [Configuration](/heal/ja/code/configuration/) | 閾値の調整、モノレポワークスペースの追加、解消ポリシーの変更をしたい        |
| [Metrics](/heal/ja/code/metrics/)             | 各メトリクスの意味と Severity の決まり方を知りたい                          |
| [Skills](/heal/ja/code/skills/)               | Claude セッションから heal を動かしたい(セットアップ、レビュー、リファクタ) |

有効化フラグはありません。`heal init` がすべての Code オブザーバを有効化した状態で `config.toml` を書きます。

## Test(オプトイン: `[features.test]`)

> _「どの本番コードがテストから見えていないか? どのテストが古くなっている、または黙って skip されているか?」_

3 つのテスト品質オブザーバを追加し、各項目に `is_test_file` フラグを付けます。中心的なシグナルは **行カバレッジ** で、外部リポータ(`cargo llvm-cov`、`pytest --cov`、`nyc`、`scoverage`)が生成した `lcov.info` を読み取ります。Hotspot のスコアにはカバレッジ未達ファイル向けの乗数が加わり、頻繁に変更され **かつ** カバレッジが不足し **かつ** 複雑なファイルが解消キューの上位に浮上します。post-commit のナッジには「N uncovered hotspot」の行が追加されるので、次にテストを書くべき場所が見えるようになります。

| ページ                                        | こんなときに読む                                                        |
| --------------------------------------------- | ----------------------------------------------------------------------- |
| [Configuration](/heal/ja/test/configuration/) | ファミリを有効化したい、または `lcov.info` を配線したい                 |
| [Metrics](/heal/ja/test/metrics/)             | 各テストシグナルが何を捕まえるかを知りたい                              |
| [Skills](/heal/ja/test/skills/)               | Claude にテストスイートをレビューさせたい、カバレッジの穴を埋めさせたい |

有効化:

```toml
[features.test]
enabled = true

[features.test.coverage]
enabled = true
```

セットアップ用のスキルに任せることもできます。ファミリを有効にし、スタックを判別して、`lcov.info` がまだ無ければカバレッジのリポータを配線します(CI の変更は提案だけです)。Claude Code で次を実行します。

```
/heal:setup tests
```

## Docs(オプトイン: `[features.docs]`)

> _「どのドキュメントが、説明している実装からずれているか?」_

ペアになったドキュメントとソースを比較する 7 つのドキュメント品質オブザーバを追加します:鮮度、識別子の参照切れ、ペアの欠落、内部リンク切れ、孤立ページ、TODO マーカーの密度、そして docs 用の Hotspot コンポーザ。各ドキュメントが説明するソースの対応表は小さな JSON ファイル(`.heal/doc_pairs.json`、`/heal:setup docs` が一度生成)で管理します。Markdown / RST の重複検出もこのファミリで有効になります。Hotspot のスコアにも、ペアになったドキュメントが古いファイルへの乗数が加わります。

| ページ                                        | こんなときに読む                                                            |
| --------------------------------------------- | --------------------------------------------------------------------------- |
| [Configuration](/heal/ja/docs/configuration/) | ファミリを有効化したい、ペアファイルの中身を知りたい                        |
| [Metrics](/heal/ja/docs/metrics/)             | 各ドキュメントシグナルが何を捕まえるかを知りたい                            |
| [Skills](/heal/ja/docs/skills/)               | Claude にペアを検出させたい、ドキュメントを監査させたい、修正を適用させたい |

有効化:

```toml
[features.docs]
enabled = true
```

このあと、セットアップ用スキルの docs の手順を 1 度実行してください(上のスイッチもあわせて入れてくれます)。ソースツリーと doc ツリーを走査して doc ⇔ source の対応関係を推論し、`.heal/doc_pairs.json` を書き出します。この対応表が無いと docs ファミリは何と何を比べればよいか分からないため、この手順は「あれば便利」ではなく「有効化の一部」です。Claude Code で次を実行します。

```
/heal:setup docs
```

## Semantic(オプトイン: `[features.semantic]`)

> _「このコード・テスト・ドキュメントは、書いてあるとおりの意味になっているか?」_

メトリクスでは答えられない問いを TypeSafe の分類モデル Jev に投げ、その答えを Code・Test・Docs の各ファミリに加えます。heal の中でネットワークに内容を送るのはこの機能だけで、それも `heal semantic ask` を実行したときだけです。何が送られるか、API キー、料金は [Semantic (Jev)](/heal/ja/semantic/) を参照してください。

```toml
[features.semantic]
enabled = true
```

## どの順序で有効化するか

典型的な導入順は次のとおりです:

1. **まず Code から。** `heal init` を実行し、`/heal:refactor` で Finding を片付けます。最初の意図的なリファクタの波で `Critical 🔥` をゼロまで持っていけば、ベースラインが整います。
2. **次に Test を追加。** `lcov.info` がある(または用意できる)なら有効化します。`coverage_pct` と `skip_ratio` の Finding によって、「テストを追加しよう」が順序付きキューに変わります。
3. **最後に Docs を追加。** ドキュメントのドリフトが頻繁に起きるようになったら有効化します。Layer A のペアリングには `/heal:setup docs` を 1 回実行する必要がありますが、それ以降は `heal status` のたびに doc ファミリも走ります。

オプトインのファミリは後から無効化できます。`enabled = false` にすれば、次の `heal status --refresh` で対応する項目が TODO リストから消えます。再度有効化すれば再 calibrationなしで戻ってきます。
