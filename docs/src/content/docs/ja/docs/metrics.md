---
title: Docs · メトリクス
description: "[features.docs] ファミリが生む 6 つのドキュメント品質メトリクスと、Markdown 重複検出。"
---

オプトインの **Docs** ファミリは、常時オンの Code ファミリの上に 6 つのメトリクスを追加します。各メトリクスは、ドキュメントが説明している実装からずれていく特定のしかたを狙い撃ちします。

設定の調整値は [Docs › 設定](/heal/ja/docs/configuration/)、同梱スキルは [Docs › スキル](/heal/ja/docs/skills/) を参照。

## 一覧

| メトリクス | レイヤ | 何を捕まえるか | Severity |
|---|---|---|---|
| `doc_freshness` | A(ペア) | ペアになったドキュメントが最後に変わって以降のソースコミット数 | 設定可能フロア(デフォルト ≥ 5 High、≥ 20 Critical) |
| `doc_drift` | A(ペア) | ドキュメントが、ペアソースに存在しない識別子を参照 | 一律 Critical |
| `doc_coverage` | A(ペア) | ペアエントリの `doc` パスがディスク上に存在しない | 一律 Medium |
| `doc_link_health` | A + B | 内部の相対パス / `#anchor` リンクが解決しない | 一律 High |
| `orphan_pages` | B(standalone) | どこからもリンクされていない Layer B ドキュメント。ペアでもない | 一律 Medium |
| `todo_density` | A + B | ドキュメント単位の `TODO` / `FIXME` / `XXX` / `TBD` / `[要確認]` / `[要修正]` カウント | ≥ 3 Medium、≥ 10 High |

**Layer A**(ペアドキュメント)と **Layer B**(standalone prose docs)は別々のオブザーバスコープです。Layer A は `.heal/doc_pairs.json` の doc ⇔ src マッピングが必要で、`/heal-doc-pair-setup` が生成します。Layer B は `standalone` の include / exclude グロブで自動発見されます。

## `doc_freshness`

> _「ドキュメントが最後に触れられて以降、ソースは動いたか?」_

ペアごとの「ペアドキュメントが最後に変わってからのソースコミット数」です。距離は **git commit** で測るので、チームのコミットペースが変わってもしきい値はずれません。

Severity は `[features.docs.doc_freshness]` の絶対フロアで決まります:

| `src_commits_since_doc ≥` | Severity(デフォルト) |
|---|---|
| 20 | Critical |
| 5  | High |
| 1  | Medium |
| 0  | Ok(Finding は出ない) |

調整方法は [Docs › 設定](/heal/ja/docs/configuration/#features.docsdoc_freshness)。

## `doc_drift`(Type 1: 識別子の参照切れ)

> _「ドキュメントは、まだソースに存在する識別子を参照しているか?」_

各 Layer A ドキュメントを走査し、識別子形のバックティック span(`` `Foo::bar` ``、`` `processOrder` ``)を抽出して、ペアソースのどの識別子にも解決しないものごとに Finding を出します。

**Severity:** Critical。存在しない識別子に従って動く読者は、もう存在しないコードを探す時間を失います。修正は機械的(参照を消すか、新しい名前で識別子を復活させる)です。

**v0.4 でのスコープ外**(v0.5+ に延期):

- Type 2 — シグネチャミスマッチ。関数はあるがパラメータがドキュメントの例と一致しない。
- Type 3 — 意味的ドリフト。関数は同じシグネチャで存在するが、ドキュメントの説明が何をするかについて間違っている。

## `doc_coverage`

> _「ペアドキュメントは実際にディスクに存在するか?」_

`.heal/doc_pairs.json` のペアエントリで `doc` パスがディスクに存在しないものです。Severity は **一律 Medium** にしています。Critical にすると、空のスタブを書いてメトリクスを満たすインセンティブを生んでしまうため(1 行ファイルを書くだけのため)です。

Medium は「これを書くことを検討してください」と言います。「書かなければなりません」とは言いません。修正は本物のコンテンツを書くか、「ドキュメントを書かない」という意思決定を `heal mark accept` で記録するか、のどちらかです。

## `doc_link_health`

> _「ドキュメント内の内部リンクは解決するか?」_

Layer A ドキュメントと Layer B の standalone walk を走査し、次のいずれかごとに Finding を発行します:

- `MissingPath` — 相対パスリンクがディスク上のファイルに解決しない。
- `MissingAnchor` — 同ドキュメント `#anchor`(または別ドキュメント `path.md#anchor`)が target の heading にマッチしない。

Heading の slug は GitHub 互換の slugify 規約(lowercase + non-alnum → `-`)に従います。

**Severity:** High — 内部破損は機械的に修正でき、reader への影響が大きいためです。

**スコープ外:** 外部 HTTP リンク。heal はローカル限定で、HTTP は CI 上で `lychee` などが担当します。

## `orphan_pages`

> _「どの Layer B ドキュメントがどこからも到達されないか?」_

`[features.docs.standalone]` 配下の Layer B ドキュメントのうち、他のどの Layer B ドキュメントからもリンクされておらず、ペアでもないもの(Layer A ペアはペアファイル経由で暗黙的に到達可能)です。

慣習的なエントリポイントは「リンク済み」として seed されるので、メトリクスを誤って引きません:

- 任意の深さの `README.md`。
- 任意の深さの `index.md`。

両方ともドキュメントグラフの外(ディレクトリリスト、docs サイトの index ページ)から到達できるので、トップレベルだからといって orphan として表面化すべきではありません。

**Severity:** Medium。orphan ドキュメントは壊れているわけではなく、見つけにくいだけです。修正は通常、親 README からの 1 行リンクで、書き直しではありません。

## `todo_density`

> _「各ドキュメントが open な TODO をいくつ抱えているか?」_

ドキュメント単位の `TODO` / `FIXME` / `XXX` / `TBD` / `[要確認]` / `[要修正]` マーカー数です。fenced コードブロック内のマーカーは除外します(説明用の例で、本物のアクション項目ではないため)。

| `marker_count ≥` | Severity |
|---|---|
| 10 | High |
| 3  | Medium |
| ≤ 2 | Ok(Finding なし) |

しきい値は v0.4 ではハードコードです。今後のリリースで設定調整値になる可能性があります。

## Markdown 重複

`[features.docs]` を有効にすると、Duplication オブザーバが Markdown / RST ファイルに対する並列パスを追加します。Findings はコード側の重複ブロックと同じ `duplication` メトリクス文字列の下に着地し、区別はファイル拡張子です。

トークン化はコードパスとは異なります: word-split + lowercase 化、fenced コードブロックは剥がします。これにより prose トークンと code トークンが衝突しません。

ユースケース: 言語ミラー(en + ja)間、モジュール固有の README 間、共有ボイラープレートを持つ API リファレンスページ間で、コピペされたドキュメントを発見すること。修正は通常、「see also」リンク + 単一の正規ソースです。

ウィンドウ長は `[metrics.duplication].docs_min_tokens`(デフォルト 100)です。詳しくは [Docs › 設定](/heal/ja/docs/configuration/#markdown--rst-重複ウィンドウ)。

## `doc_hotspot` — どの paired doc を次に直すべきか

`doc_hotspot` は code Hotspot の docs ファミリ版です。**paired** な doc ↔ src エントリを `paired_src_churn × debt` でランクします:

```
debt = src_commits_since_doc + weight_drift × dangling_idents
```

ペアスコアが高い = ペアになっている src が高頻度で変わっており、**かつ** doc が追いついていない(commits-since-doc、dangling identifier、またはその両方)。「全 doc のうち次に更新する価値があるのはどれか」を表します。

スコア対象は `doc_pairs.json` の paired エントリのみです。standalone な散文(README、コンセプトガイド)はスコープ外で、`orphan_pages` と `todo_density` がそれぞれのシグナルでカバーします。

`doc_hotspot` 自体は常に `Severity::Ok` です。docs ファミリの Finding(`doc_freshness`、`doc_drift`、`doc_coverage`、`doc_link_health`、`todo_density`)を doc 側にも paired src 側にも装飾するので、`docs/api.md` に立った `doc_drift` Finding と同じペアの `doc_freshness` Finding が同時に `hotspot=true` を拾います。

デフォルトの卒業ゲートは `[features.docs.hotspot] floor_ok = 5`(おおむね「2 commits × 2 debt 単位」)。`weight_drift` のデフォルトは `1.0` で、factually wrong な doc(dangling identifier)を merely stale なものより優先したい場合は値を上げてください(例: `5.0`)。

## `/heal-doc-review` と `/heal-doc-patch` がこれらをどう使うか

`/heal-doc-review` は `heal status --json` を読み、docs ファミリにフィルタリングし、findings を **Diátaxis** のレンズ(Tutorial / How-to / Reference / Explanation)でフレーム化します:

- Tutorial / How-to のドリフトを最初に(混乱した初心者ユーザーが最高レバレッジの修正対象)。
- Reference のドリフトを次に(オーディエンスが高頻度)。
- Explanation のドリフトを最後に(時間的緊急度は低い)。

`/heal-doc-patch` は docs スライスのキャッシュを 1 finding 1 commit で drain します:

- **`doc_link_health`** → リンクを修正する(相対パスまたは anchor slug)。
- **`doc_drift`** → 古い識別子の参照を消す、または明確なリネームがあれば新しい名前で識別子を復活させる。
- **`doc_freshness`** → ペアソースを再読み、ドキュメントを更新する。
- **`orphan_pages`** → 親 README からのリンクを足す、または orphan を削除する。
- **`todo_density`** → 解決可能な TODO を解決し、残りは issue にエスカレートする。
- **`doc_coverage`** → スタブドキュメントを書く、またはチームがこのソースはドキュメントを必要としないと決めたなら `heal mark accept`。

詳しくは [Docs › スキル](/heal/ja/docs/skills/) を参照。
