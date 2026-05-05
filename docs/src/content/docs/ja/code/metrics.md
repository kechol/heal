---
title: Code · メトリクス
description: Code ファミリの 7 メトリクス、Severity ラダー、最初に見るべき Hotspot の役割。
---

Code ファミリは 7 つのメトリクスを同梱しています。どれも AI 専用ではなく、長年の文献に裏打ちされたコードヘルス指標です。heal の貢献は **コードベース自身の分布に合わせて calibrate すること**。200 行のスクリプトと 200kloc のサービスは、同じ生の値でも違う扱いを受けます。

## Severity ラダー

各 Finding には `Critical` / `High` / `Medium` / `Ok` のいずれかが付きます。判定は 2 段階です。

1. **絶対フロア**(文献由来): `value ≥ floor_critical` → Critical、`value < floor_ok` → Ok。全体的に劣化したコードベースでもワーストケースが Critical のまま、きれいなコードベースではパーセンタイルのループに縛られず卒業できる、というガード。
2. **コードベース自身のパーセンタイル**(2 つのフロアの間): `≥ p95` Critical、`≥ p90` High、`≥ p75` Medium、それ未満は Ok。

パーセンタイルは `.heal/calibration.toml` に保存され、`heal calibrate` で再計算されます。フロアは `[metrics.<m>]` で上書きできます — [設定](/heal/ja/code/configuration/)を参照。

## 解消ティア

`heal status` は Ok 以外の Finding を `[policy.drain]` 駆動で 3 つのティアにグループ化します:

- **T0 — 解消キュー**(デフォルト `["critical:hotspot"]`) — `/heal-code-patch` が解消する must-fix リスト。
- **T1 — 余裕があれば解消**(デフォルト `["critical", "high:hotspot"]`) — 別セクションで表示し、自動解消はしない。
- **Advisory** — それ以外の非 Ok。`--all` を付けない限り表示されない。

T0 はゴール、T1 は衛生、Advisory は余裕があれば確認、という形です。

## 各メトリクス

### LOC — Lines of Code

> _「コードベースは何でできているか?」_

[`tokei`](https://github.com/XAMPPRocky/tokei) で言語別に code / comment / blank 行をカウントします。他のメトリクスは LOC の言語検出に依存します。Markdown / Org は主言語の検出から除外されるので、ドキュメントの多いリポジトリも実装言語に解決されます。常時オン、Severity なし。

### Complexity — CCN と Cognitive

> _「読みにくい関数はどれか?」_

関数単位で同時に計算する 2 メトリクスです:

- **CCN**(Cyclomatic Complexity) — McCabe の分岐カウント。
- **Cognitive Complexity** — Sonar の可読性メトリクス。ネストの深さにペナルティを課す。

両者は独立に calibrate され、文献由来のフロア(CCN は McCabe、Cognitive は SonarQube)を使います。両方とも **プロキシメトリクス** です。`floor_ok` がきれいなコードベースをラダーから卒業させます。

**言語**: TypeScript / JavaScript / Python / Go / Scala / Rust。

### Churn — 変更頻度

> _「動いているのはどこか?」_

`since_days` ウィンドウ(デフォルト 90 日)でのファイル単位コミット数。first-parent のみ集計します。Churn 自体には Severity がなく、Hotspot と post-commit ナッジに供給されます。

### Change Coupling — 一緒に動くファイル

> _「コード上は無関係だが、いつも一緒に変わるファイルは?」_

各コミットが触ったファイル群を co-occurrence イベントとして数え、ペア単位のカウンタでインポートグラフに現れない依存を可視化します。各ペアは **Symmetric**(両方が一緒に変わる、最も強いシグナル)か **OneWay** のいずれかに分類されます。

「coupling のように見えるが実はそうでない」ペア — ロックファイルの更新、生成コード、`mod.rs ↔ 兄弟ファイル` — は自動で除外。テスト ↔ ソースおよびドキュメント ↔ ソースのペアはデフォルトで Advisory に降格しますが、`[features.test]` が ON のときはドリフトしたテストペアが `change_coupling.drift` として再昇格します([Test › メトリクス](/heal/ja/test/metrics/)を参照)。

### Duplication — コピーされたブロック

> _「重複しているのはどこか?」_

`min_tokens`(デフォルト 50)のスライディングウィンドウで構文木を歩き、同一トークン列の連なり(Type-1 clone)を検出します。フォーマット変更ではコピーは隠せませんが、変数名の変更には弱いです。

`[features.docs]` が ON のときは Markdown / RST に対する並列パスも走ります([Docs › メトリクス](/heal/ja/docs/metrics/)を参照)。

**言語**: Complexity と同じ。

### LCOM — Lack of Cohesion of Methods

> _「機械的に分割できるクラスはどれか?」_

クラスごとに、フィールド参照を共有するメソッドや互いを呼ぶメソッドをグラフのエッジとしてつなげます。連結成分の数が LCOM 値で、`cluster_count ≥ 2` ならそのクラスは責務がほどけて分かれそうな状態 — Extract Class の候補です。

現在の構文ベースのバックエンドには既知の盲点があります(継承、動的プロパティアクセスなど)。表面化したクラスは「自動で分割」ではなく「人間がレビューする候補」として扱ってください。

**言語**: TypeScript / JavaScript / Python / Rust のクラススコープ。(Go にはクラススコープがなく、Scala は LSP バックエンド待ちです。)

## Hotspot

Hotspot は churn × complexity を掛け合わせて、読みにくく頻繁に編集されるファイルを浮かび上がらせます。出力は Severity ティアではなく、ファイル単位のフラグ(スコア分布の上位 10%)で、レンダラはそのファイルの他の Finding の上に `🔥` 絵文字として表示します — Finding は `Critical 🔥` / `High 🔥` / `Medium 🔥` / `Ok 🔥` のどれにもなりえます。

誰も触らない複雑なファイルは負債、チームが毎日のように編集する複雑なファイルは次のバグが生まれる場所、と考えてください。デフォルトの解消キュー `(critical:hotspot)` がまさにその交差点です。だからこそ Hotspot は heal が出す **最もアクション可能なシグナル** です。

「Ok 🔥」サブセット — 低 Severity だが頻繁に編集されている、「なぜまだここを編集しているのか?」候補 — は `heal status --all` の専用セクションに現れます。

詳しい背景は [コンセプト › Hotspot](/heal/ja/concept/#hotspot--次にバグが生まれそうなファイルを指し示す印) を参照。
