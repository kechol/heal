---
title: Docs · スキル
description: "[features.docs] 向け同梱 Claude Code スキル 3 種 — /heal-doc-pair-setup、/heal-doc-review、/heal-doc-patch。"
---

オプトインの **Docs** ファミリは Claude スキルを 3 種同梱しています。`heal skills install` / `heal init` で Code ファミリスキルと並んで展開されますが、docs オブザーバが生む findings にしか作用しません。

インストール手順とドリフト認識付きの更新の仕組みは [Code › スキル](/heal/ja/code/skills/) を参照(共通の仕組みです)。

## `/heal-doc-pair-setup` — ペアファイルを書く

ワンショットセットアップスキルです。ソースツリーとドキュメントツリーをスキャンし、doc ⇔ src ペアを検出し、既存ペアファイル(手動エントリは保持)とマージし、`.heal/doc_pairs.json` をアトミックに書き出します。

heal はこのファイルの決定論的な **消費者** で、検出ロジックを持ちません。だから生成は `heal status` 内ではなく、このユーザトリガースキルにあります。

### このスキルが正しいタイミング

- `[features.docs]` を有効化した直後の初回セットアップ。heal は `.heal/doc_pairs.json` の欠落を警告し、ユーザをここに誘導します。
- コードベースの構造が変わり(新しいモジュール、ドキュメントツリーの再編)、既存のペアリストが明らかなペアを取りこぼしているとき。
- ユーザが手動でペアエントリを追加したく、ファイルのスキーマを尋ねるとき。

### 3 つの heuristic

| Heuristic | ペアを選ぶ方法 |
|---|---|
| **Mention** | ドキュメント本体が `path/to/source.rs` または単一の src ファイルに解決するバックティック span 識別子を参照している。 |
| **Mirror** | ディレクトリ構造ミラー: `docs/payments/engine.md` ↔ `src/payments/engine.ts`。 |
| **LLM**(任意) | 上記 2 つが失敗したときに LLM でドキュメント + 候補ソースを読む。デフォルトはスキップ。スキルは呼び出し前に確認します。 |

各候補は `confidence: 0.0–1.0` と `source` を持ちます。マージパスはすべての `source: "manual"` エントリをそのまま残します。

### 何を書くか

`.heal/doc_pairs.json` のみ。ソースファイルに対しては読み取り専用です。`[features.docs]` のオブザーバが次に走るときに、`heal status` と `heal metrics` がこれを読み返します。

トリガーフレーズ: 「set up doc pairs」、「generate doc_pairs.json」、「initialize heal docs」、「/heal-doc-pair-setup」。

## `/heal-doc-review` — 監査スキル

読み取り専用です。`heal status --json` を読み、`[features.docs]` スライスにフィルタリングし、次の 2 つを返します:

1. ドキュメントツリーの **アーキテクチャ的読解**。支配的な軸は「Tutorial が実際のインストール手順からドリフトした」「API リファレンスが古い」「コンセプトドキュメントのリンクが切れている」のどれか?
2. **優先順位付きドキュメント修正 TODO リスト** — まず Tutorial / How-to のドリフト(混乱した初心者ユーザーが最高レバレッジの修正対象)、次に Reference、最後に Explanation。

メトリクス別の **Diátaxis** レンズでのフレーミング:

| メトリクス | Diátaxis 観点 |
|---|---|
| `doc_freshness` | ユーザが最初に読むセクションが動いたか? |
| `doc_drift` | このドキュメントからコピペされたスニペットはまだコンパイルできるか? |
| `doc_coverage` | このソースのオーディエンスは、そもそもドキュメントを見つけることを期待されているか? |
| `doc_link_health` | 内部ナビゲーションは動くか? |
| `orphan_pages` | このページはオーディエンスが使うエントリポイントから到達可能か? |
| `todo_density` | このドキュメントは活発に建設中か、それとも静かに見捨てられているか? |

`/heal-doc-review` は提案のみで、ソースは編集しません。書き込み側のカウンターパートは `/heal-doc-patch` です。

トリガーフレーズ: 「review the docs health」、「where should we fix documentation」、「/heal-doc-review」。

## `/heal-doc-patch` — 書き込みスキル

`.heal/findings/latest.json` の docs スライスを 1 件ずつ drain します。**1 修正 1 コミット** です。

事前チェック(失敗すると起動拒否):

1. クリーンな worktree。
2. キャッシュ存在(欠けていれば `heal status --json` を走らせて埋めます)。
3. `[features.docs]` 有効(`.heal/config.toml` で)。
4. `doc_freshness` / `doc_drift` / `doc_coverage` の finding がスコープ内なら、`.heal/doc_pairs.json` が存在すること。無ければ `/heal-doc-pair-setup` に誘導します。

メトリクス別の drain パターン:

| メトリクス | デフォルトの手 |
|---|---|
| `doc_link_health`(`MissingPath`) | 相対パスを更新する。target がリネームされていれば `git log --diff-filter=R` でリネームを追う。 |
| `doc_link_health`(`MissingAnchor`) | heading slug を合わせる。heading がリネームされていれば、リンクを新しい slug に更新する。 |
| `doc_drift` | 古い識別子の参照を消す、または `git log -S` で明確なリネームが見つかれば新名前で識別子を復活させる。 |
| `orphan_pages` | 親 README からのリンクを足す。orphan を削除すべきならユーザにエスカレートする。 |
| `todo_density` | 解決可能な TODO(例: 「TODO: API ref へリンク」が ref が存在するようになって解決可能)を解決し、残りは GitHub issue にエスカレートしてリンクをドキュメントに残す。 |
| `doc_freshness` | ペアソースを再読み、影響を受けたドキュメントセクションを書き直す。書き直しは voice と構造を保持する(これは内容同期で再設計ではない)。 |
| `doc_coverage` | ユーザにエスカレートする。patch スキルは一方的にまったく新しいドキュメントを書かない(空スタブを避けるため)。 |

スキル本体に encoded された refusal:

- **stub-without-content** — `doc_coverage` を黙らせるためだけの 1 行ファイルは書きません。本物のコンテンツかエスカレート、のどちらか。
- **cosmetic-pass** — コミット単位の修正は特定の Finding を狙います。drive-by の文章書き直しや「ついでに」の reformatting はしません。
- **doc-as-truth-source** — ドリフトしたドキュメントに合わせてコードを更新することはしません。ソースが canonical で、ドキュメントが更新される側です。

スキルが強制する制約:

- 1 finding = 1 commit。
- Conventional Commit の subject + body + `Refs: F#<finding_id>` トレーラ。
- push しない、amend しない、`--no-verify` しない。

`/heal-doc-patch` は Code または Test ファミリのメトリクスに属する findings をスキップします。

トリガーフレーズ: 「fix the doc findings」、「drain the doc cache」、「patch stale docs」、「/heal-doc-patch」。
