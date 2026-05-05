---
title: Docs · メトリクス
description: '[features.docs] ファミリが追加するドキュメント品質メトリクス 7 種と、Markdown 重複検出。'
---

オプトインの **Docs** ファミリは、常時オンの Code ファミリの上に 7 つのメトリクスを追加します。各メトリクスは、ドキュメントが説明している実装からずれていく特定のしかたを狙い撃ちします。

設定の調整値は [Docs › 設定](/heal/ja/docs/configuration/)、同梱スキルは [Docs › スキル](/heal/ja/docs/skills/) を参照。

## 一覧

| メトリクス        | レイヤ        | 何を捕まえるか                                                                             | Severity                                   |
| ----------------- | ------------- | ------------------------------------------------------------------------------------------ | ------------------------------------------ |
| `doc_freshness`   | A(ペア)       | ペアドキュメントが最後に変わって以降のソースコミット数                                     | フロア(デフォルト ≥ 5 High、≥ 20 Critical) |
| `doc_drift`       | A(ペア)       | ドキュメントがペアソースに存在しない識別子を参照                                           | 一律 Critical                              |
| `doc_coverage`    | A(ペア)       | ペアエントリの `doc` パスがディスク上に存在しない                                          | 一律 Medium                                |
| `doc_link_health` | A + B         | 内部の相対パス / `#anchor` リンクが解決しない                                              | 一律 High                                  |
| `orphan_pages`    | B(standalone) | どこからもリンクされていない Layer B ドキュメント。ペアでもない                            | 一律 Medium                                |
| `todo_density`    | A + B         | ドキュメント単位の `TODO` / `FIXME` / `XXX` / `TBD` / `[要確認]` / `[要修正]` カウント     | ≥ 3 Medium、≥ 10 High                      |
| `doc_hotspot`     | A(ペア)       | `paired_src_churn × debt` の合成スコア。docs ファミリの Finding に `hotspot=true` を立てる | 常に Ok(装飾キャリア)                      |

**Layer A**(ペアドキュメント)は `.heal/doc_pairs.json` の doc ⇔ src マッピングが必要で、`/heal-doc-pair-setup` が生成します。**Layer B**(standalone prose docs)は `[features.docs.standalone]` の include / exclude グロブで自動発見されます。

## `doc_freshness`

> _「ドキュメントが最後に触れられて以降、ソースは動いたか?」_

ペアごとの「ペアドキュメントが最後に変わってからのソースコミット数」です。距離は **git commit** で測るので、チームのコミットペースが変わってもしきい値はずれません。フロアは `[features.docs.doc_freshness]` にあります — デフォルトは `≥ 1` Medium、`≥ 5` High、`≥ 20` Critical。

## `doc_drift`(Type 1: 識別子の参照切れ)

> _「ドキュメントは、まだソースに存在する識別子を参照しているか?」_

各 Layer A ドキュメントを走査し、識別子形のバックティック span(`` `Foo::bar` ``、`` `processOrder` ``)を抽出して、ペアソースのどの識別子にも解決しないものごとに Finding を出します。**Severity: Critical** — 存在しない識別子に従って動く読者は、もう存在しないコードを探す時間を失うため。修正は機械的(参照を消すか、新しい名前で識別子を復活させる)です。

Type 2(シグネチャミスマッチ)と Type 3(意味的ドリフト)は v0.5+ に延期しています。

## `doc_coverage`

> _「ペアドキュメントは実際にディスクに存在するか?」_

ペアエントリで `doc` パスがディスクに存在しないものです。**Severity: 一律 Medium**。Critical にすると空のスタブを書くだけのインセンティブを生むため、Medium で「これを書くことを検討してください」に留めます。修正は本物のコンテンツを書くか、「ドキュメントを書かない」という意思決定を `heal mark accept` で記録するか、のどちらか。

## `doc_link_health`

> _「ドキュメント内の内部リンクは解決するか?」_

リンク単位の Finding: `MissingPath`(相対パスがファイルに解決しない)と `MissingAnchor`(`#anchor` が target の heading にマッチしない)。Heading の slug は GitHub 互換の規約(lowercase + non-alnum → `-`)に従います。**Severity: High** — 内部破損は機械的に修正でき、reader への影響が大きいため。

外部 HTTP リンクはスコープ外。heal はローカル限定で、HTTP は CI 上で `lychee` などが担当します。

## `orphan_pages`

> _「どの Layer B ドキュメントがどこからも到達されないか?」_

他のどの Layer B ドキュメントからもリンクされておらず、ペアでもない Layer B ドキュメントです。慣習的なエントリポイント(任意の深さの `README.md` / `index.md`)は「リンク済み」として seed されるので、メトリクスを誤って引きません。**Severity: Medium** — orphan ドキュメントは壊れているわけではなく見つけにくいだけ。修正は通常、親 README からの 1 行リンクで済みます。

## `todo_density`

> _「各ドキュメントが open な TODO をいくつ抱えているか?」_

ドキュメント単位の `TODO` / `FIXME` / `XXX` / `TBD` / `[要確認]` / `[要修正]` マーカー数。fenced コードブロックとバッククォート inline-code 内のマーカーはデフォルトで除外するので、マーカーキーワード自体を _説明_ するリファレンスページ(まさにこのドキュメント)が段落ごとに自爆しません。**Severity:** `≥ 3` Medium、`≥ 10` High。

## Markdown 重複

`[features.docs]` を有効にすると、Duplication オブザーバが Markdown / RST ファイルに対する並列パスを追加します。Findings はコード側の重複ブロックと同じ `duplication` メトリクス文字列の下に着地し、区別はファイル拡張子です。

ユースケースは、言語ミラー(en + ja)間・モジュール固有の README 間・共有ボイラープレートを持つ API リファレンス間でコピペされたドキュメントを発見すること。修正は通常「see also」リンク + 単一の正規ソースです。

ウィンドウ長は `[metrics.duplication].docs_min_tokens`(デフォルト 100)。

## Doc Hotspot — どのドキュメントを次に直すべきか

Doc Hotspot は code Hotspot の docs ファミリ版です。**paired** な doc ↔ src エントリを `paired_src_churn × debt` でランクします(`debt = src_commits_since_doc + weight_drift × dangling_idents`)。スコアが高い = ペアになっている src が高頻度で変わっており、**かつ** doc が追いついていない、という意味です。「全 doc のうち次に更新する価値があるのはどれか」を表します。

スコア対象は `doc_pairs.json` の paired エントリのみです。standalone な散文(README、コンセプトガイド)はスコープ外で、`orphan_pages` と `todo_density` がそれぞれのシグナルでカバーします。

Doc Hotspot 自体は常に `Severity::Ok` です。docs ファミリの Finding(`doc_freshness`、`doc_drift`、`doc_coverage`、`doc_link_health`、`todo_density`)を装飾し、`🔥` フラグが「次に直すべき doc-pair」を指し示します。

## 解消パターン

`/heal-doc-review` は **Diátaxis** のレンズで findings をフレーム化します — Tutorial / How-to のドリフトを最初に(混乱した初心者ユーザーが最高レバレッジの修正対象)、次に Reference、最後に Explanation。`/heal-doc-patch` は docs スライスを 1 件 1 コミットで消化します:

- **`doc_link_health`** → 相対パスまたは anchor slug を修正。
- **`doc_drift`** → 古い識別子の参照を消す、または明確なリネームがあれば新しい名前で復活。
- **`doc_freshness`** → ペアソースを再読み、ドキュメントを更新。
- **`orphan_pages`** → 親 README からのリンクを足す、または orphan を削除。
- **`todo_density`** → 解決可能な TODO を解決し、残りは issue へ。
- **`doc_coverage`** → 本物のスタブを書く、またはこのソースはドキュメントを必要としないとチームが判断したなら `heal mark accept`。

詳しい契約は [Docs › スキル](/heal/ja/docs/skills/) を参照。
