---
title: メトリクス
description: heal がコミットごとに収集するメトリクスと、それぞれの意味、Severity の付け方、使いどころ。
---

heal には現在 7 つのメトリクスが付属しています。いずれも AI 専用で
はなく、何十年も使われてきた実績のあるコードヘルスメトリクスです。
heal の貢献は **コードベース自身の分布に合わせてキャリブレーション
する** ところにあります — 200 行のスクリプトと 200kloc のサービス
を、同じ生の値で別々にトリガーし、その結果を `/heal-code-fix` が消化す
る TODO リストとして提示します。

このページでは各メトリクスを概観します。設定項目については
[設定](/heal/ja/configuration/)を参照。

## Severity ラダー

すべての Finding は次の 4 段階のいずれかに収まります。

| Tier      | ルール                                                                                |
| --------- | ------------------------------------------------------------------------------------- |
| Critical  | `value ≥ floor_critical` または `value ≥ p95`（calibrate された 95 パーセンタイル）。 |
| High      | `value ≥ p90`                                                                         |
| Medium    | `value ≥ p75`                                                                         |
| Ok        | それ以外                                                                              |

`floor_critical` のデフォルト値は確立された文献（McCabe / SonarQube）
から取られています: CCN 25、Cognitive 50、Duplication 30%。パーセ
ンタイル区切り（`p75 / p90 / p95`）は calibrate 時のコードベース自
身の分布から計算され、`.heal/calibration.toml` に保存されます。

**Hotspot は直交します。** Severity ではなくフラグ（Hotspot スコア
分布の上位 10%）です。Finding は `Critical 🔥`（構造的に悪く、かつ
よく触られている）にも `Critical`（構造的に悪いが静か）にもなり得ま
す — レンダラーは別バケットとして並べます。

## 推奨される導入順

1. **LOC** は常に有効。プライマリ言語検出が他のオブザーバーすべて
   を駆動します。
2. **CCN** + **Cognitive** はデフォルト有効。calibrate により絶対
   フロアの上にコードベース固有のしきい値を載せます。
3. **Churn** は有効。参照ウィンドウ内に 10 件ほどコミットが溜まる
   と意味のある情報になります。
4. **Hotspot** は単独メトリクスとしてもっとも実用的 — 有効のままに。
5. **Change Coupling**（one-way + symmetric）と **Duplication** は
   診断用です。有効のまま、問題調査時に確認してください。
6. **LCOM** は機械的に分割可能なクラスを浮かび上がらせます。リファ
   クタ候補の発見に有用 — 有効のままに。

## LOC — Lines of Code

> _「このコードベースは何で構成されているのか？」_

[`tokei`](https://github.com/XAMPPRocky/tokei) を使って、言語ごとの
コード行数、コメント行数、空行をカウントします。LOC は基礎的なメト
リクスです — 他のメトリクスはこれによる言語検出に依存します（複雑
度はパースできる言語でしか動かず、Hotspot はコミットを複雑度で重み
付けします）。

「primary language」は、リテラート言語ではない言語のうちもっともコー
ド行数が多いものです。ドキュメント中心のリポジトリでも実装言語に解
決されるよう、Markdown と Org は意図的に除外されています。

LOC は常に有効でトグルはありません。`tokei` がファイル単位でキャッ
シュするため、コストはほぼ無視できます。

## Complexity — CCN と Cognitive

> _「どの関数が追いにくいのか？」_

tree-sitter のシングルウォークで計算される、関数単位の 2 つのメト
リクスです。

- **CCN**（Cyclomatic Complexity） — McCabe によるブランチ数。
  各 `if`、`for`、`while`、`case`、`&&`、`||`、`?` ごとに 1 増えま
  す。
- **Cognitive Complexity** — Sonar の可読性メトリクス。**ネストの
  深さ**を罰し（深くなるほど加算量が増える）、連鎖した論理演算子は
  単一の加算にまとめます。

両メトリクスは独立に calibrate されます。

- CCN: `floor_critical = 25`（McCabe の "untestable in practice"）。
- Cognitive: `floor_critical = 50`（SonarQube Critical のベースライン）。

**対応言語**: TypeScript と Rust。JS / Python / Go / Scala は今後
のリリースで追加されます。

## Churn — ファイルがどれだけ変更されているか

> _「何が動いているのか？」_

ファイルごとのコミット数と追加・削除行数の合計を、直近
`since_days` ウィンドウ（デフォルト 90 日）で算出します。マージコ
ミットの二重カウントを避けるため first-parent 履歴を使います。

Churn が高いこと自体は必ずしも問題ではありません — `package.json`
は頻繁に変わって当然です。Churn は複雑度と組み合わせたときに意味を
持ちます（[Hotspot](#hotspot--churn--complexity)を参照）。

Churn は専用の Severity ラダーを持ちません。Hotspot と post-commit
ナッジに供給されます。

## Change Coupling — 一緒に動くファイル

> _「どのファイルがどのファイルに、暗黙のうちに依存しているか？」_

各コミットで、そのコミットが触ったパス集合が 1 つの共起イベントに
なります。ペア単位のカウンタにより、インポートグラフ上では見えない
暗黙の依存関係が浮かび上がります。`min_coupling`（デフォルト 3）を
超えたペアが Finding になります。

生のカウンタに加え、各ペアは **Symmetric** か **OneWay** に分類さ
れます。

- **Symmetric**: `min(P(B|A), P(A|B)) ≥ symmetric_threshold`（デフォ
  ルト 0.5）。両方のファイルが互いなしで変更されることがほとんどな
  い — このメトリクスでもっとも強い「責務の混在」シグナルです。
- **OneWay { from, to }**: `from` は単独でも変更されるが、`to` は
  ほぼ常にお供する。partner により条件依存が強い側を `to` に選びま
  す。

Symmetric なペアはメトリクスタグ `change_coupling.symmetric` の下
に出力され、レンダラーは汎用カウンタより強いシグナルとして見えるよ
う分離します。

**バルクコミットのキャップ**: 50 ファイル超を触るコミットは完全に
スキップされます。一括フォーマットが無関係なファイル間に偽の結合を
作るのを防ぐためです。

## Duplication — コピーされたブロック

> _「どこに重複があるのか？」_

tree-sitter の構文木を歩いて、`min_tokens`（デフォルト 50）サイズ
のトークンウィンドウをマッチさせ、長く続く同一トークン列（Type-1
クローン）を見つけます。整形やホワイトスペースの違いではクローンは
隠れません。一方、変数のリネームは隠れます。

calibrate にはファイル単位の重複率分布を使います。
`floor_critical = 30%`（ファイルの 1/3 が重複なら構造的な問題）。

**対応言語**: 複雑度と同じ（TypeScript、Rust）。

## Hotspot — Churn × Complexity

> _「リグレッションはどこに集中するか？」_

Adam Tornhill が広めた「コードを犯罪現場として見る」視点です。
Hotspot はファイルのコミット数（Churn）に、関数の CCN 合計（複雑度）
を掛け合わせます。

```
score = (weight_complexity × ccn_sum) × (weight_churn × commits)
```

スコアの高いファイルは、頻繁に変更されており、かつ読みにくい — 歴
史的にリグレッションが集中するところです。

重みはどちらもデフォルト `1.0`。Hotspot は独立したパーセンタイル空
間を使い、`score ≥ p90` で **フラグ** を立てます。レンダラーはその
ファイルの Finding に `🔥` 絵文字を付加します。Severity Tier では
**ありません** — Hotspot ファイルは Critical 🔥、High 🔥、
Medium 🔥、Ok 🔥 のいずれにもなり得ます。最後の 1 つ「Severity は
低いがよく触られている、なぜまだ編集している？」候補は、
`heal check --all` の専用セクションで追加表示されます。

式は乗算であるため、複雑度は高いが直近のコミットがないファイルはス
コアが 0 になります — Hotspot は _アクティブな_ トラブルを特定する
ためのものであり、過去の負債を掘り返すためのものではありません。

## LCOM — Lack of Cohesion of Methods

> _「どのクラスが機械的に分割可能か？」_

クラスごと（TS の `class_declaration`、Rust の `impl_item`）に、
heal は無向グラフを構築します。同じ `this.foo` / `self.foo` フィー
ルドを参照しているメソッドは接続され、兄弟メソッドへの呼び出しは直
接エッジになります。連結成分の数が LCOM 値です。

- `cluster_count == 1`: クラスは凝集している。
- `cluster_count ≥ 2`: クラスは分離可能な責務を持つ。各クラスタは
  原理的にそれぞれ別の型になり得る。

デフォルトの `min_cluster_count = 2` は Severity 分類の前に凝集し
たクラスを除外します。calibrate された `cluster_count` 分布が実際
の Tier を割り当てます。

**近似の注意点**（`backend = "tree-sitter-approx"`）:

- 基底クラスから継承されたフィールドは見えません。
- 動的プロパティアクセス（`this[name]`）は見えません。
- メソッド間で共有されるヘルパー関数がクラス外にあると、メソッドは
  無関係に見えます。

これらは false positive 寄りに偏ります — 表示されるクラスは人間レ
ビューの候補であり、自動判断には使いません。型情報を使う
`backend = "lsp"` 実装は v0.5+ に含まれます。

**対応言語**: TypeScript の class スコープ、Rust の impl ブロック。
モジュールスコープの LCOM（Rust ファイルレベルの自由関数、TS の
named-export グループ）は先送りです。

## heal がこれらをどう使うか

コミットごとに、heal は次を行います。

1. 全オブザーバーを実行（`run_all` 1 回パス）。
2. `MetricsSnapshot`（`severity_counts` 含む）を
   `.heal/snapshots/` に書き出す。
3. すべての Critical / High Finding を post-commit ナッジで stdout
   に出す。

`heal check` はオンデマンドで分析を再実行し、Finding を Severity で
分類して `CheckRecord` を `.heal/checks/latest.json` に書き出しま
す。このキャッシュを `/heal-code-fix` スキルが 1 コミット 1 Finding ずつ
消化していきます。

スクリプティング向けに、JSON の正確な形と保存方法は
[アーキテクチャ › スナップショット](/heal/ja/architecture/#snapshots--メトリクスペイロード)
にまとめています。
