---
title: メトリクス
description: heal がコミットごとに収集するメトリクスと、それぞれの意味、Severity の付け方、なぜ採用しているのか。
---

heal には現在 7 つのメトリクスが付属しています。いずれも AI 専用ではなく、何十年も使われてきた実績のある定番です。heal の貢献は **コードベース自身の分布に合わせて calibrate する** こと — 200 行のスクリプトと 200kloc のサービスを同じ生の値で別々にトリガーする — そして結果を `/heal-code-patch` が消化する TODO リストとして提示することにあります。

このページは Severity の付け方 → finding のバケット分け → 各メトリクスの中身 → Hotspot という特別ケース → CCN/Cognitive を扱う注意、の順に並んでいます。設定項目は[設定](/heal/ja/configuration/)を参照。

## Severity ラダー

すべての Finding は `Critical / High / Medium / Ok` のいずれかに分類されます。判定は **2 段階** で行います。

**Stage 1 — 絶対フロア（文献由来）。** パーセンタイル分類器を両端で正気に保つための逃げ道です。

| ルール                   | 結果     | 理由                                                                                                                                              |
| ------------------------ | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `value ≥ floor_critical` | Critical | 一律に悪いコードベースでもワーストケースは Critical のまま（CCN 25、Cognitive 50、Dup 30%）。                                                     |
| `value < floor_ok`       | Ok       | 卒業ゲート（proxy メトリクスのみ）— クリーンなコードベースが「上位 10% は永遠に赤」のループに縛られないように。デフォルトは CCN 11、Cognitive 8。 |

**Stage 2 — コードベース自身のパーセンタイル分布。** 2 つのフロアの間に落ちる値は、calibrate 時に取った分布での位置で分類されます。

| ルール        | 結果     |
| ------------- | -------- |
| `value ≥ p95` | Critical |
| `value ≥ p90` | High     |
| `value ≥ p75` | Medium   |
| それ以外      | Ok       |

パーセンタイル区切りは `.heal/calibration.toml` に保存されます。両フロアはメトリクスごとに config で上書き可能です。[設定 › Floors](/heal/ja/configuration/#floors) を参照。

## Drain tier

`heal status` は非 Ok の Finding を `[policy.drain]` 駆動で 3 段階に振り分けます。

- **T0 — Drain queue**（デフォルト `["critical:hotspot"]`）。`/heal-code-patch` が drain する must-fix リスト。
- **T1 — Should drain**（デフォルト `["critical", "high:hotspot"]`）。余裕があれば対処、別セクションに表示、自動 drain はしない。
- **Advisory** — それ以外の非 Ok。`--all` 指定時のみ表示。

この分離が「drain to zero」を意味のあるものにします。T0 がゴール、T1 が衛生、Advisory は余裕のあるときの review。proxy である CCN が T0 に入るのは hotspot で裏付けられたときのみ — そうでないと Goodhart ループに陥ります。詳細は[設定 › Drain ポリシー](/heal/ja/configuration/#drain-ポリシー)を参照。

## 各メトリクス

6 つのオブザーバーがコミットごとに走ります。7 つ目の **Hotspot** はそれらを合成します。各オブザーバーの設定項目は[設定](/heal/ja/configuration/)に集めてあります。

### LOC — Lines of Code

> _「このコードベースは何で構成されているのか？」_

[`tokei`](https://github.com/XAMPPRocky/tokei) を使って言語ごとのコード行数、コメント行数、空行をカウントします。LOC は基礎的なメトリクスです。他のメトリクスはこれによる言語検出に依存します（複雑度はパースできる言語でしか動かず、Hotspot はコミットを複雑度で重み付けします）。

「primary language」はリテラート言語ではない言語のうちもっともコード行数が多いものです。ドキュメント中心のリポジトリでも実装言語に解決されるよう、Markdown と Org は意図的に除外されています。

LOC は常に有効でトグルはありません。`tokei` がファイル単位でキャッシュするため、コストはほぼ無視できます。

### Complexity — CCN と Cognitive

> _「どの関数が追いにくいのか？」_

tree-sitter のシングルウォークで計算される、関数単位の 2 つのメトリクスです。

- **CCN**（Cyclomatic Complexity）— McCabe によるブランチ数。各 `if`、`for`、`while`、`case`、`&&`、`||`、`?` ごとに 1 増えます。
- **Cognitive Complexity** — Sonar の可読性メトリクス。**ネストの深さ** を罰し（深くなるほど加算量が増える）、連鎖した論理演算子は単一の加算にまとめます。

両メトリクスは独立に calibrate されます。

- CCN: `floor_critical = 25`（McCabe "untestable"）、`floor_ok = 11`（McCabe "simple"）。
- Cognitive: `floor_critical = 50`（SonarQube Critical baseline）、`floor_ok = 8`（Sonar — "review" 閾値の半分）。

`floor_ok` 未満の関数はパーセンタイルに関係なく Ok 固定です。理由は下の[CCN と Cognitive はなぜ _proxy_ なのか](#ccn-と-cognitive-はなぜ-proxy-なのか)を参照。

**対応言語**: TypeScript、JavaScript、Python、Go、Scala、Rust。

### Churn — ファイルがどれだけ変更されているか

> _「何が動いているのか？」_

ファイルごとのコミット数と追加・削除行数の合計を、直近 `since_days` ウィンドウ（デフォルト 90 日）で算出します。マージコミットの二重カウントを避けるため first-parent 履歴を使います。

Churn が高いこと自体は必ずしも問題ではありません — `package.json` は頻繁に変わって当然です。Churn は複雑度と組み合わせたときに意味を持ちます（[Hotspot](#hotspot) を参照）。

Churn は専用の Severity ラダーを持ちません。Hotspot と post-commit ナッジに供給されます。

### Change Coupling — 一緒に動くファイル

> _「どのファイルがどのファイルに、暗黙のうちに依存しているか？」_

各コミットで、そのコミットが触ったパス集合が 1 つの共起イベントになります。ペア単位のカウンタにより、インポートグラフ上では見えない暗黙の依存関係が浮かび上がります。`min_coupling`（デフォルト 3）を超えたペアが Finding になります。

生のカウンタに加え、各ペアは **Symmetric** か **OneWay** に分類されます。

- **Symmetric**: `min(P(B|A), P(A|B)) ≥ symmetric_threshold`（デフォルト 0.5）。両方のファイルが互いなしで変更されることがほとんどない — このメトリクスでもっとも強い「責務の混在」シグナルです。
- **OneWay { from, to }**: `from` は単独でも変更されるが、`to` はほぼ常にお供する。partner により条件依存が強い側を `to` に選びます。

Symmetric なペアはメトリクスタグ `change_coupling.symmetric` の下に出力され、レンダラーは汎用カウンタより強いシグナルとして見えるよう分離します。

**バルクコミットのキャップ**: 50 ファイル超を触るコミットは完全にスキップされます。一括フォーマットが無関係なファイル間に偽の結合を作るのを防ぐためです。

### Duplication — コピーされたブロック

> _「どこに重複があるのか？」_

tree-sitter の構文木を歩いて `min_tokens`（デフォルト 50）サイズのトークンウィンドウをマッチさせ、長く続く同一トークン列（Type-1 クローン）を見つけます。整形やホワイトスペースの違いではクローンは隠れません。一方、変数のリネームは隠れます。

calibrate にはファイル単位の重複率分布を使います。`floor_critical = 30%`（ファイルの 1/3 が重複なら構造的な問題）。

**対応言語**: 複雑度と同じ（TypeScript、JavaScript、Python、Go、Scala、Rust）。

### LCOM — Lack of Cohesion of Methods

> _「どのクラスが機械的に分割可能か？」_

クラスごと（TypeScript / JavaScript の `class_declaration`、Python の `class_definition`、Rust の `impl_item`）に、heal は無向グラフを構築します。同じ `this.foo` / `self.foo` フィールドを参照しているメソッドは接続され、兄弟メソッドへの呼び出しは直接エッジになります。連結成分の数が LCOM 値です。

- `cluster_count == 1`: クラスは凝集している。
- `cluster_count ≥ 2`: クラスは分離可能な責務を持つ。各クラスタは原理的にそれぞれ別の型になり得る。

デフォルトの `min_cluster_count = 2` は Severity 分類の前に凝集したクラスを除外します。calibrate された `cluster_count` 分布が実際の Tier を割り当てます。

**近似の注意点**（`backend = "tree-sitter-approx"`）:

- 基底クラスから継承されたフィールドは見えません。
- 動的プロパティアクセス（`this[name]`）は見えません。
- メソッド間で共有されるヘルパー関数がクラス外にあると、メソッドは無関係に見えます。

これらは false positive 寄りに偏ります — 表示されるクラスは人間レビューの候補であり、自動判断には使いません。型情報を使う `backend = "lsp"` 実装は v0.5+ に含まれます。

**対応言語**: TypeScript / JavaScript の class スコープ、Python の
class スコープ、Rust の impl ブロック。Go は class スコープを持た
ないため非対応、Scala は class / trait / object / case class の
表現力ゆえに LSP バックエンド（v0.5+）待ちです。

## Hotspot

Hotspot は特別です。単独のメトリクスではなく、**他のメトリクスの上に載るレバレッジ乗数** です。

> _「リグレッションはどこに集中するか？」_

Adam Tornhill が広めた「コードを犯罪現場として見る」視点です。Hotspot はファイルのコミット数（Churn）に、関数の CCN 合計（複雑度）を掛け合わせます。

```
score = (weight_complexity × ccn_sum) × (weight_churn × commits)
```

出力は **Severity Tier ではなく**、ファイル単位のフラグ（スコア分布の上位 10%）です。レンダラーはそのファイルの finding に `🔥` を付加します。Finding は `Critical 🔥`、`High 🔥`、`Medium 🔥`、`Ok 🔥` のいずれにもなり得ます。

これに専用セクションを置く理由は、Hotspot が heal が出力する **もっとも実用的な単一シグナル** だからです。誰も触らない複雑なファイルは負債、毎日のように編集されている複雑なファイルは次のバグが生まれる場所です。デフォルトの drain queue（`critical:hotspot`）が存在するのは、その交差点こそリファクタの 1 分が一番返ってくるからです。

式は乗算なので、複雑度は高いが直近のコミットがないファイルはスコアが 0 になります — Hotspot は **アクティブな** トラブルを特定するためのものであり、過去の負債を掘り返すためのものではありません。

「Ok 🔥」サブセット — Severity は低いがよく触られている、「なぜまだ編集している？」候補 — は `heal status --all` の専用セクションで追加表示されます。

## CCN と Cognitive はなぜ _proxy_ なのか

McCabe (1976) は CCN を「ブランチカバレッジに必要なテストケース数の下限を見積もる静的指標」として導入しました — コード品質そのものを定義するためではありません。Sonar の Cognitive Complexity (2017) も可読性の代理指標です。これらをゼロに向けて drain しようとすると可読性が損なわれます。

- 手続き的に凝集した関数に Extract Function を当てると、CCN は新ヘルパーへ relocate するだけでグローバル合計は減らない。
- フラットな positive composite（`if (A && B && C)`）を否定 guard チェーンに変換しても、元がネストしていなければ Cognitive は下がらず、読み手の負荷はむしろ増える。

heal の設計はこれを受け入れています。`floor_ok` で proxy メトリクスからの卒業を許容し、Hotspot で「触られている」ファイルへのレバレッジを増幅し、drain tier モデルで TODO を「proxy と本質的問題が一致する Finding」に絞ります。詳細は `/heal-code-review` スキルの `architecture.md` §6 のトラップカタログを参照。

## heal がこれらをどう使うか

コミットごとに、heal は次を行います。

1. post-commit フックが全オブザーバーを 1 回走らせる。
2. Critical / High の finding を stdout に出力する — デーモンなしで「次の一手」が常に見えます。

`heal status` はオンデマンドで分析を再実行し、Severity 別に分類して `.heal/findings/` に TODO リストを書き出します。これを `/heal-code-patch` が 1 コミット 1 finding で消化していきます。同じ `(commit, config, calibration)` に対する再実行はキャッシュヒットで無料です。
