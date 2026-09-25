---
title: Docs · メトリクス
description: '[features.docs] ファミリが出す、ドキュメントの質を見る 7 つのメトリクスと、Markdown の重複検出。'
---

オプトインの **Docs** ファミリは、常時オンの Code ファミリに 7 つのメトリクスを加えます。どれも、ドキュメントが説明しているソースからずれていく、特定の形を狙っています。

設定項目は [Docs › 設定](/heal/ja/docs/configuration/) を、スキルは [Docs › スキル](/heal/ja/docs/skills/) を参照してください。

## 一覧

| メトリクス        | レイヤ          | 何を見つけるか                                                                             | Severity                                           |
| ----------------- | --------------- | ------------------------------------------------------------------------------------------ | -------------------------------------------------- |
| `doc_freshness`   | A（ペア）       | ペアのドキュメントが最後に変わってから入った、ソースのコミット数                           | フロア（既定は 5 以上で High、20 以上で Critical） |
| `doc_drift`       | A（ペア）       | ペアのソースにもう存在しない識別子を、ドキュメントが参照している                           | 一律 Critical                                      |
| `doc_coverage`    | A（ペア）       | ペアのエントリの `doc` のパスが、ディスクにない                                            | 一律 Medium                                        |
| `doc_link_health` | A + B           | 内部の相対パスや `#anchor` のリンクが解決しない                                            | 一律 High                                          |
| `orphan_pages`    | B（standalone） | どこからもリンクされておらず、ペアにもなっていない Layer B のドキュメント                  | 一律 Medium                                        |
| `todo_density`    | A + B           | ドキュメントごとの `TODO` / `FIXME` / `XXX` / `TBD` / `[要確認]` / `[要修正]` の数         | 3 件以上で Medium、10 件以上で High                |
| `doc_hotspot`     | A（ペア）       | `paired_src_churn × debt` の合成スコア。docs ファミリの Finding に `hotspot=true` を立てる | 常に Ok（装飾を運ぶだけ）                          |

**Layer A**（ペアのドキュメント）には、`.heal/doc_pairs.json` にドキュメントとソースの対応が必要です。これは `/heal:setup docs` が作ります。**Layer B**（standalone の文章のドキュメント）は、`[features.docs.standalone]` の include / exclude のグロブで自動的に見つけます。

## `doc_freshness`

> _「ドキュメントを最後に触ってから、ソースは動いたか?」_

ペアごとの、ペアのドキュメントより後に入ったソースのコミット数です。実際の日数ではなく **git のコミット数**で測るので、チームのコミットのペースが変わっても、しきい値はずれません。フロアは `[features.docs.doc_freshness]` にあり、既定では `≥ 1` で Medium、`≥ 5` で High、`≥ 20` で Critical です。

## `doc_drift`（Type 1: 識別子の参照切れ）

> _「ドキュメントは、ソースに今もある識別子を参照しているか?」_

Layer A の各ドキュメントを走査して、識別子の形をしたバッククォートの範囲（`` `Foo::bar` ``、`` `processOrder` ``）を取り出し、ペアのソースのどの識別子にも当たらないものごとに Finding を出します。**Severity: Critical**。存在しない識別子を頼りに動いた読み手は、もうないコードを探して時間を失うからです。直し方は機械的で、参照を消すか、識別子を新しい名前で書き直します。

Type 2（シグネチャの不一致）は、まだ実装していません。Type 3（意味のずれ）は、[Semantic (Jev)](/heal/ja/semantic/) を有効にすると `doc_drift.semantic` として出ます。

## `doc_coverage`

> _「ペアのドキュメントは、本当にディスクにあるか?」_

`doc` のパスが存在しないペアのエントリです。**Severity は一律 Medium** にしています。Critical にすると、中身のない空のページを量産する動機を生んでしまうからです。Medium は「書くことを考えてみては」という意味で、「書かなければならない」ではありません。直すには、中身のあるページを書くか、「ドキュメントは書かない」という判断を `heal mark accept` で記録します。

## `doc_link_health`

> _「ドキュメントの内部リンクは解決するか?」_

リンクごとに Finding を出します。`MissingPath`（相対パスのリンクがファイルに当たらない）と `MissingAnchor`（`#anchor` がリンク先のどの見出しにも一致しない）です。見出しのスラッグは GitHub と同じ規則（小文字にして、英数字以外は `-` にする）に従います。**Severity: High**。内部のリンク切れは機械的に直せるうえ、読み手への影響が大きいからです。

外部の HTTP リンクは扱いません。heal はローカルだけで動くツールで、HTTP の確認は CI で `lychee` などが受け持ちます。

## `orphan_pages`

> _「どこからもたどり着けない Layer B のドキュメントはどれか?」_

ほかのどのドキュメントからもリンクされておらず、ペアにもなっていない Layer B のドキュメントです。慣習的な入口（どの階層の `README.md` や `index.md` も）は、最初から「リンクされている」ものとして扱うので、このメトリクスには引っかかりません。**Severity: Medium**。孤立したドキュメントは壊れているわけではなく、見つけにくいだけだからです。たいていは、親の README から 1 行リンクすれば直ります。

## `todo_density`

> _「各ドキュメントは、未解決の TODO をいくつ抱えているか?」_

ドキュメントごとの `TODO` / `FIXME` / `XXX` / `TBD` / `[要確認]` / `[要修正]` の数です。フェンスで囲んだコードブロックの中の印（既定では、バッククォートで囲んだインラインコードの中の印も）は数えません。印のキーワードそのものを*説明する*リファレンスのページが、段落ごとに自分で引っかからないようにするためです。**Severity:** `≥ 3` で Medium、`≥ 10` で High。

## Markdown 重複

`[features.docs]` がオンのとき、Duplication のオブザーバは Markdown / RST のファイルにも同じ検出を並行して行います。Finding は、コードのブロックと同じ `duplication` というメトリクス名で出ます。見分けるのはファイルの拡張子です。

使いどころは、ページのあいだでコピーされたドキュメントを見つけることです。たとえば、言語ごとのミラー（英語と日本語など）、モジュールごとの README、決まり文句を共有した API リファレンスのページのあいだで起こります。たいていは、「あわせて読む」のリンクを張り、正本を 1 つにまとめれば直ります。

窓の長さは `[metrics.duplication].docs_min_tokens`（既定 100）です。

## Doc Hotspot — 次に直す価値が高いドキュメント

Doc Hotspot は、code の Hotspot の docs ファミリ版です。**ペア**になったドキュメントとソースのエントリを、`paired_src_churn × debt` でランク付けします。ここで `debt = src_commits_since_doc + weight_drift × dangling_idents` です。ソースの変化が速く、**しかも**ドキュメントが取り残されているペアほど、スコアが高くなります。「すべてのドキュメントのうち、次に更新する価値が高いのはこれ」という意味です。

スコアを付けるのは、`doc_pairs.json` にあるペアのエントリだけです。standalone のドキュメント（README、コンセプトの説明）は、代わりに `orphan_pages` と `todo_density` が扱います。

Doc Hotspot 自体は常に `Severity::Ok` です。docs ファミリの Finding（`doc_freshness`、`doc_drift`、`doc_coverage`、`doc_link_health`、`todo_density`）を装飾し、同じ `🔥` が次に更新する価値の高いペアを指すようにします。

## 解消のしかた

`/heal:docs` は、Finding を **Diátaxis** の観点で整理します。ただし並び順は、HEAL が決めたとおりに保ちます。有効な Tier、Severity、docs ファミリ内の `hotspot_score` の降順で、スコアのないものは最後、同点ならメトリクス、パス、id の順です。チュートリアル / ハウツー / リファレンス / 解説という分類は、診断と直し方を決めるのに使い、キューの優先度には使いません。承認された提案は、1 コミットずつ適用します。

- **`doc_link_health`** → 相対パスかアンカーのスラッグを直す。
- **`doc_drift`** → 古い参照を消す。名前の変更がはっきりしていれば、識別子を新しい名前で書き直す。
- **`doc_freshness`** → ペアのソースを読み直し、ドキュメントを更新する。
- **`orphan_pages`** → 親の README からリンクを張るか、孤立したページを消す。
- **`todo_density`** → 解決できる TODO を片付け、残りは issue にする。
- **`doc_coverage`** → 中身のあるひな形を書く。そのソースには専用のドキュメントが要らないとチームで決めたなら、`heal mark accept` で記録する。

詳しい約束ごとは [Docs › スキル](/heal/ja/docs/skills/) を参照してください。
