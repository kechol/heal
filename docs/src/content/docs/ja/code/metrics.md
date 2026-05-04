---
title: Code · メトリクス
description: heal が収集する 7 つのコードヘルスメトリクス、Severity ラダー、drain ティアの考え方、そして Hotspot を最初に見るべき理由。
---

heal の常時オン Code ファミリは 7 つのメトリクスを同梱しています。どれも AI 専用ではなく、長年の文献に裏打ちされたコードヘルス指標です。heal の貢献は **コードベース自身の分布に合わせてキャリブレーションすること**。200 行のスクリプトと 200kloc のサービスは、同じ生の値でも違う扱いを受けます。

このページでは、Severity の決まり方、findings のバケット分け、各メトリクスの意味、Hotspot の特殊な役割を順に説明します。設定の調整値は [Code › 設定](/heal/ja/code/configuration/) を参照してください。

## Severity ラダー

各 Finding には `Critical` / `High` / `Medium` / `Ok` のいずれかが付きます。判定は 2 段階です。

**第 1 段階 — 絶対フロア(文献由来)。** 両端でパーセンタイル分類器を健全に保つ脱出口です:

| ルール                   | 結果     | 理由                                                                                       |
| ------------------------ | -------- | ------------------------------------------------------------------------------------------ |
| `value ≥ floor_critical` | Critical | 一様に劣化したコードベースでもワーストケースは Critical のまま(CCN 25、Cognitive 50、Dup 30%)。|
| `value < floor_ok`       | Ok       | プロキシメトリクス向けの卒業ゲート。クリーンなコードベースが「上位 10% は永遠に赤」に縛られないため。|

**第 2 段階 — コードベース自身のパーセンタイル分布。** 2 つのフロアの間に落ちるものは、キャリブレーション時にキャプチャした分布での位置で分類します:

| ルール        | 結果     |
| ------------- | -------- |
| `value ≥ p95` | Critical |
| `value ≥ p90` | High     |
| `value ≥ p75` | Medium   |
| それ以外      | Ok       |

パーセンタイル区切りは `.heal/calibration.toml` にあります。フロアはメトリクスごとに設定で上書きできます。

## Drain ティア

`heal status` は Ok 以外の Finding を `[policy.drain]` 駆動で 3 つのティアにグループ化します:

- **T0 — Drain queue**(デフォルト `["critical:hotspot"]`)。`/heal-code-patch` が drain する must-fix リストです。
- **T1 — Should drain**(デフォルト `["critical", "high:hotspot"]`)。帯域に余裕があれば。別セクションで表示し、自動 drain はしません。
- **Advisory** — それ以外で Ok 以上のもの。`--all` を付けない限り表示されません。

この区切りが「ゼロまで drain」を意味あるものにします。T0 はゴール、T1 は衛生、Advisory は余裕があれば確認、という形です。CCN は **プロキシ** 指標なので、hotspot で裏付けられない限り T0 には属しません。さもないと Goodhart のループに陥ります。

## 各メトリクス

毎コミット 6 つのオブザーバが走り、7 番目の **Hotspot** がそれらを合成します。設定の詳細は [Code › 設定](/heal/ja/code/configuration/) を参照。

### LOC — コード行数

> _「このコードベースは何でできているか?」_

[`tokei`](https://github.com/XAMPPRocky/tokei) を使って言語別にコード・コメント・空白行をカウントします。LOC は基盤メトリクスで、他のメトリクスは LOC の言語検出に依存しています(complexity は parse できる言語にしか走らない、hotspot は commits を complexity で重み付けする、など)。

「主言語」はコード行数が最大の非 literate 言語です。Markdown と Org は意図的に除外しているので、ドキュメント中心のリポジトリでも実装言語に解決されます。

LOC は常に有効でトグルはありません。

### Complexity — CCN と Cognitive

> _「どの関数が追いにくいか?」_

1 回のパスで関数ごとに 2 つのメトリクスを計算します:

- **CCN**(Cyclomatic Complexity) — McCabe 流の分岐カウント。各 `if`、`for`、`while`、`case`、`&&`、`||`、`?` で 1 加算。
- **Cognitive Complexity** — Sonar の可読性メトリクス。ネストの深さにペナルティを与え(深くなるほど加算が大きい)、論理演算子のチェーンを 1 回の加算にまとめます。

両者は独立にキャリブレーションされます:

- CCN: `floor_critical = 25`(McCabe「テスト不能」)、`floor_ok = 11`(McCabe「単純」)。
- Cognitive: `floor_critical = 50`(SonarQube Critical)、`floor_ok = 8`(Sonar の review 閾値の半分)。

`floor_ok` を厳密に下回る関数はパーセンタイルに関係なく Ok に分類されます。理由は下記の [なぜ CCN と Cognitive はプロキシなのか](#なぜ-ccn-と-cognitive-はプロキシなのか) を参照。

**対応言語**: TypeScript、JavaScript、Python、Go、Scala、Rust。

### Churn — ファイルの変更頻度

> _「何が動いているか?」_

最新の `since_days` ウィンドウ(デフォルト 90)でのファイルごとのコミット数と追加 / 削除行数。マージコミットの二重カウントを避けるため first-parent 履歴を使います。

high-churn なファイル自体は問題ではありません。`package.json` は頻繁に変わって当然です。Churn が意味を持つのは complexity と組み合わさったとき([Hotspot](#hotspot))です。

Churn 単体は Severity ラダーを持ちません。Hotspot と post-commit ナッジに供給されます。

### Change Coupling — 一緒に動くファイル

> _「どのファイルが暗黙のうちに依存し合っているか?」_

各コミットで触ったパスの集合が 1 つの co-occurrence イベントになります。ペアごとのカウンタが、import グラフでは見えない暗黙の依存を浮き上がらせます。

各ペアは **Symmetric** か **OneWay** にも分類されます:

- **Symmetric** — 両方のファイルが互いなしには変わりにくい。最も強い「責務の混在」シグナルです。
- **OneWay { from, to }** — `from` は単独でよく変わるが、`to` はほぼ常についていきます。

Symmetric ペアは `change_coupling.symmetric` で表示されます。レンダラはこれを分離して、汎用カウンタより強いシグナルとして可視化します。

**ノイズフィルタ。** 一見 coupling に見えるけれど実は違うペア(lockfile の更新、生成成果物、`mod.rs ↔` 兄弟ファイル)は自動的に捨てるか Advisory に降格します。テスト ↔ ソース、ドキュメント ↔ ソースのペアは「一緒に動くのが当然」なのでデフォルトで Advisory に降格します。`[features.test]` を有効にすると、ソースから **遅れている** テスト ↔ ソースペアは Medium の `change_coupling.drift` に再昇格します(詳細は [Test › Metrics](/heal/ja/test/metrics/#change_couplingdrift))。

**バルクコミットキャップ。** 50 ファイルを超えるコミットは完全にスキップします。大規模リフォーマットが無関係なファイル間の coupling を捏造しないようにするためです。

### Duplication — コピーされたブロック

> _「重複はどこにあるか?」_

parse ツリーを歩いて、サイズ `min_tokens`(デフォルト 50)のトークンウィンドウを照合し、同一トークンの長い連続(Type-1 クローン)を見つけます。リフォーマットや空白の変更ではクローンを隠せませんが、変数のリネームでは隠せます。

キャリブレーションはファイル単位の重複率分布を使います。`floor_critical = 30%`(ファイルの 1/3 が重複なら構造的問題)。

`[features.docs]` を有効にすると、独自のウィンドウ長で Markdown / RST に対する並列パスが走ります。詳しくは [Docs › Metrics](/heal/ja/docs/metrics/#markdown-重複)。

**対応言語**: complexity と同じ(TypeScript、JavaScript、Python、Go、Scala、Rust)。

### LCOM — メソッドの凝集度欠如

> _「どのクラスが機械的に分割可能か?」_

クラスごとに、heal は `this.foo` / `self.foo` のフィールド参照を共有するメソッドを連結する無向グラフを構築します(兄弟メソッド呼び出しは直接エッジ)。連結成分の数が LCOM 値です。

- `cluster_count == 1` — クラスは凝集している。
- `cluster_count ≥ 2` — クラスは分離可能な責務を持つ。各クラスタは原理的にはそれぞれ独自の型になれます。

デフォルトの `min_cluster_count = 2` が凝集したクラスを Severity 分類前に取り除きます。

**近似に伴う注意**(現在の構文ベースバックエンド):

- ベースクラスから継承したフィールドは見えません。
- 動的プロパティアクセス(`this[name]`)は見えません。
- メソッド間で共有されるヘルパー関数がクラス外にあると、メソッド同士が無関係に見えます。

これらは false positive 寄りのバイアスを生みます。表面化したクラスは人間によるレビュー候補で、自律判断の対象ではありません。型認識バックエンドは v0.5+ ロードマップにあります。

**対応言語**: TypeScript / JavaScript のクラススコープ、Python のクラススコープ、Rust の impl ブロック。Go にはクラススコープがなく、Scala は型認識バックエンドの導入を待ちます。

## Hotspot

Hotspot は特別です。単独のメトリクスではなく、**他のメトリクスの上に乗る乗算器** です。

> _「リグレッションはどこに集中するか?」_

Adam Tornhill が広めた「コード = 犯罪現場」の見方です。Hotspot はファイルのコミット数(churn)に、その関数の CCN 合計(complexity)を掛け合わせます:

```
score = (weight_complexity × ccn_sum) × (weight_churn × commits)
```

出力は **Severity ティアではなく**、ファイル単位のフラグ(スコア分布の上位 10%)で、レンダラはそのファイルの他の finding の上に `🔥` 絵文字として表示します。Finding は `Critical 🔥`、`High 🔥`、`Medium 🔥`、あるいは `Ok 🔥` でもありえます。

Hotspot は heal が出す **最もアクション可能なシグナル** です。誰も触らない複雑なファイルは負債、チームが毎日のように編集する複雑なファイルは次のバグが生まれる場所、と考えてください。デフォルトの drain キュー(`critical:hotspot`)が存在するのは、その交差点こそリファクタの 1 分が一番返ってくる場所だからです。

式は乗算なので、complexity が高くても最近のコミットがないファイルはスコアがゼロになります。Hotspot は **アクティブな** トラブルを特定するためのもので、歴史的負債のためではありません。

「Ok 🔥」サブセット(低 Severity だが頻繁に編集されている、「なぜまだここを編集しているのか?」候補)は `heal status --all` の専用セクションに現れます。

### 任意ブースト(Test / Docs ファミリ)

docs / test ファミリを有効にすると、Hotspot のスコアにペアになったドキュメントが古いファイルや行カバレッジが不足しているファイル向けの小さな乗数が加わります。両ブーストは単一の **1.5× 上限** を共有するので、複数の軸で悪いファイルがシグナル蓄積だけで単軸悪いファイルを上回ることはありません。

詳しくは [Docs › Metrics](/heal/ja/docs/metrics/#hotspot--doc-drift-ブースト) と [Test › Metrics](/heal/ja/test/metrics/#hotspot--カバレッジブースト)。

## なぜ CCN と Cognitive はプロキシなのか

McCabe(1976)は CCN を「分岐網羅に必要な最小テストケース数」の静的推定として導入しました。コード品質メトリクスではありません。Sonar の Cognitive Complexity(2017)は可読性のプロキシです。どちらもゼロに向けて駆動すると可読性を損ないます:

- 手続き的に凝集している関数に Extract Function を当てると、CCN が減るのではなく場所が移るだけです。
- フラットな肯定形複合(`if (A && B && C)`)を否定形ガードチェーンに変換しても Cognitive は動かず(元がネストしていない)、しばしば読み手の負担を **増やします**。

heal の設計はこれを受け入れています。`floor_ok` がクリーンなコードベースをプロキシメトリクスから卒業させます。Hotspot が編集中のファイルにレバレッジを乗算します。drain ティアモデルが TODO リストを「プロキシと根本問題が一致する finding」に絞ります。

## heal はこれらをどう使うか

毎コミット:

1. post-commit フックがすべてのオブザーバを 1 回走らせます。
2. Critical / High の findings が stdout に出ます。次の問題はデーモンなしで見え続けます。
3. `[features.test.coverage]` が有効なら、追加で uncovered hotspot のカウント行が出ます。

`heal status` がオンデマンドで分析を再実行し、Severity で findings を分類し、TODO リストを `.heal/findings/` に書き出します。`/heal-code-patch` が drain するのはそれです。同じ commit + config + calibration での再実行はキャッシュヒットで無料です。
