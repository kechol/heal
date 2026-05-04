---
title: Test · メトリクス
description: "[features.test] ファミリが生む 3 つのテスト品質メトリクス — coverage_pct、skip_ratio、change_coupling.drift と、Hotspot スコアへの影響。"
---

オプトインの **Test** ファミリは、常時オンの Code ファミリの上に 3 つのメトリクスを追加します。中心的なシグナルは **行カバレッジ** で、外部生成された `lcov.info` を読み取り、Hotspot のスコアに反映させてカバレッジ未達の hot path が drain キューの上位に浮かぶようにします。

設定の調整値は [Test › 設定](/heal/ja/test/configuration/)、同梱スキルは [Test › スキル](/heal/ja/test/skills/) を参照。

## 一覧

| メトリクス | レイヤ | 何を捕まえるか |
|---|---|---|
| `coverage_pct` | ソースファイル単位 | `lcov.info` から読んだ行カバレッジ。Finding は `< 100%` のファイルにのみ発行 |
| `skip_ratio` | テストファイル単位 | ファイル内の skip テスト数 / 総テスト数(%) |
| `change_coupling.drift` | ペア単位 | ソースだけが先に変わっていて、テストがついていけていないペア |

加えて構造的な追加: 各 Finding に `is_test_file: bool` フラグが付き、スキルがテスト側と本番側の severity を独立に読めるようになります。

## `coverage_pct`

> _「テストスイートから見えていない本番コードはどこか?」_

`[features.test.coverage].lcov_paths` の最初に存在する `lcov.info` から parse される、ソースファイル単位の行カバレッジ率です。リーダーは `cargo llvm-cov`、`pytest --cov`、`nyc`、`scoverage` の各方言を扱います。重複する SF レコード(一部のリポータがテストエントリポイントごとに 1 つ吐く)も合計ではなく max-of でマージするので、重なるカバレッジが二重カウントされません。

Finding は `< 100%` のファイルにのみ発行されます。フルカバレッジのファイルからはノイズ Finding を出しません。

### Severity の決まり方

キャリブレーションは **反転値**(`100 - coverage_pct`)を保存するので、他のメトリクスと同じ「value が p95 に達したら Critical」のカスケードがそのまま使えます。`heal calibrate --force` を実行するまでは、文献由来のフォールバックが使われます:

| カバレッジ | Severity(デフォルト) |
|---|---|
| ≤ 5%  | Critical |
| ≤ 15% | Critical(p95 経由) |
| ≤ 30% | High(p90 経由) |
| ≤ 50% | Medium(p75 経由) |
| > 75% | Ok(フロア経由) |

フロアの上書きは `config.toml` 側で行います([Test › 設定](/heal/ja/test/configuration/#キャリブレーション))。

### スコープ外

heal はテストを実行しません。**永遠に** スコープ外なのは: flakiness、ランタイム傾向、隔離性問題、mutation score など、テストスイートを実際に走らせて初めて分かること全般です。それらは CI が担当します。heal は lcov 成果物に対して読み取り専用のままです。

## `skip_ratio`

> _「skip テストの比率が無視できないファイルはどれか?」_

テストファイル単位の skip 比率(skip 数 / 総テスト数、パーセンテージ)です。`[features.test].test_paths` でマッチしたファイルを歩き、言語別の skip マーカーをカウントします: Rust の `#[ignore]`、Python の `@pytest.mark.skip` / `@unittest.skipIf`、JS / TS の `it.skip` / `xit` / `xdescribe`、Go の `t.Skip()`、ScalaTest の `ignore` / `pending`。

検出は構造的なので、コメントや文字列リテラル中のマーカーで false positive が出ることはありません。

### Severity の決まり方

`[calibration.skip_ratio]` でキャリブレーションされます。`heal calibrate --force` を実行するまでは、フォールバックは:

| Skip 比率 | Severity(デフォルト) |
|---|---|
| < 0.5% | Ok |
| > 1%   | Medium |
| > 5%   | High |
| > 10%  | Critical |
| > 20%  | Critical(フロア経由) |

Finding は **少なくとも 1 つ** skip されたテストがあるファイルにのみ発行されます。

## `change_coupling.drift`

> _「カバーするソースについていけていないテストはどれか?」_

`[features.test]` を有効にすると、本来一緒に動くべきなのに動いていないテスト ↔ ソースのペアが、Advisory ではなく real な Finding として表面化します。

テスト ↔ ソースのペアの合算 co-change カウントがプロジェクトの中央値(`change_coupling.p50`)を **下回る** ときに、`change_coupling.expected`(Advisory)から `change_coupling.drift`(Medium)に再タグ付けされます。「テストは存在するが、ソースの最近の変更がすべてテストなしで起きている」という意味で読んでください。

ドキュメント ↔ ソースのペアは drift に昇格しません。drift はテスト品質シグナルだからです。

## `test_hotspot` — 検証されないまま積み上がっている変更がどこか

`test_hotspot` は code Hotspot の test ファミリ版です。src ファイルを `commits × uncov_pct` でランクします。スコアが高い = そのファイルは編集が続いている **かつ** 大部分がテストされていない、という意味です。「検証されない変更」の文献アンカーは、test の観点からは CCN より直接的です。30 commits ある低 CCN の config-loader でカバレッジ 0% なら本物のテスト対象ですが、code Hotspot は CCN が低いせいで取りこぼします。

```
score = commits_in_90d × (100 - line_coverage_pct)
```

lcov に出てこないが git 履歴では触られているファイルは 100% gap(= 未テスト)として扱います。これがメトリクスの最重要ターゲットです。100% カバレッジのファイルはスコア 0 で落ちます。

`test_hotspot` 自体は常に `Severity::Ok` です。スコアの仕事は同じファイルの `coverage_pct` Finding に `hotspot=true` を立てることです。drain ターゲットは「Critical AND `hotspot=true`」のままで、test ファミリ単位にスコープされます。

デフォルトの卒業ゲートは `[features.test.hotspot] floor_ok = 25` です(おおむね「1 commit × 25% gap」 — 「カバレッジ 75% = decent」という文献アンカーから gap floor を取った値)。プロジェクトの分布に合わなければ上書きしてください。

## post-commit ナッジ:「uncovered hotspot」

```
heal: recorded · 3 critical, 7 high · heal status
         · 2 uncovered hotspot
```

カウントは `coverage_pct` finding のうち High または Critical 重大度かつ `hotspot=true` のものです。「次のテストはここに書くべき」の最短リマインダです。

`[features.test.coverage]` がオフのとき、または該当する finding がないときはこの行は出ません。

## `/heal-test-review` と `/heal-test-patch` がこれらをどう使うか

`/heal-test-review` は `heal status --json` を読み、test ファミリにフィルタリングし、findings をテストピラミッドのレンズ(unit / integration / e2e)でフレーム化します。

`/heal-test-patch` はテストスライスのキャッシュを 1 finding 1 commit で drain します:

- **`coverage_pct`** → カバレッジ未達の hot path に対する unit test を書く / 拡張する。
- **`skip_ratio`** → 理由が成立しなくなった skip を再有効化する、またはなぜ skip のままかを文書化する。
- **`change_coupling.drift`** → ドリフトしたテストとソースを一緒に表面化し、テストをソースの現在の形に合わせる。

スキルに encoded された refusal: assertion-weakening、skip-the-flake、scaffold-without-running。詳しい契約は [Test › スキル](/heal/ja/test/skills/) を参照。
