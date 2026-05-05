---
title: Docs · スキル
description: "[features.docs] 向け同梱 Claude Code スキル 4 種 — /heal-doc-pair-setup、/heal-doc-scaffold、/heal-doc-review、/heal-doc-patch。"
---

オプトインの **Docs** ファミリは Claude スキルを 4 種同梱しています。`heal skills install` で Code ファミリスキルと並んで展開され、原則として docs オブザーバが生む findings にしか作用しません。例外は `/heal-doc-scaffold` で、`[features.docs]` を有効化していないプロジェクトでも動きます — 出力はファミリを有効化した時点で観測対象に入ります。

インストール手順とドリフト認識付きの更新の仕組みは [Code › スキル](/heal/ja/code/skills/) を参照(共通の仕組みです)。

## `/heal-doc-pair-setup` — ペアファイルを書く

ワンショットセットアップスキル。ソースツリーとドキュメントツリーをスキャンし、doc ⇔ src ペアを検出し、既存ペアファイル(手動エントリは保持)とマージし、`.heal/doc_pairs.json` をアトミックに書き出します。heal はこのファイルの決定論的な **消費者** で検出ロジックを持たないため、生成は `heal status` 内ではなくこのユーザトリガースキルにあります。

**実行タイミング:** `[features.docs]` を有効化した直後の初回セットアップ(heal がファイル欠落を警告して誘導)、コードベース構造が大きく変わって既存ペアが取りこぼしているとき、手動でペアエントリを追加したいとき。

**3 つの heuristic** でペアを選びます:

| Heuristic | ペアを選ぶ方法 |
|---|---|
| **Mention** | ドキュメント本体が `path/to/source.rs` または単一の src ファイルに解決するバックティック span 識別子を参照している。 |
| **Mirror** | ディレクトリ構造ミラー: `docs/payments/engine.md` ↔ `src/payments/engine.ts`。 |
| **LLM**(任意) | 上記 2 つが失敗したときに LLM でドキュメント + 候補ソースを読む。デフォルトはスキップ。スキルは呼び出し前に確認します。 |

各候補は `confidence` と `source` を持ち、マージパスは `source: "manual"` のエントリをそのまま残します。ソースファイルに対しては読み取り専用で、`.heal/doc_pairs.json` のみを書きます。

トリガーフレーズ: 「set up doc pairs」、「generate doc_pairs.json」、「initialize heal docs」、「/heal-doc-pair-setup」。

## `/heal-doc-scaffold` — Wiki をゼロから組む

ブートストラップスキル。何度呼び出しても安全。5 フェーズ(コードベース検出 → 既存ツリー走査 → 再構成 → 出力 → レポート)を経て、現在のコードベースシグナルをドキュメントツリーに流し込みます — 手書き編集を破壊せずに。出力先は `[features.docs] scaffold_root`(デフォルト `.heal/docs/`)配下の Markdown ツリーです。

スキルの契約:

- **検出ベースで自動判定、対話なし。** 何を出力するかは検出シグナルだけが決めます — ページ単位のメニューはなし。生成ツリーをレビューして不要ページを削除する、というレビュー 1 回で完結する設計です。
- **厳格な emit ゲート。** ページが落ちるのはコードベースから意味のあるコンテンツで埋められる時だけ。基礎ページ(README、Wiki Index、System Context、Architecture Overview、Glossary、Getting Started)は常に出力。条件付きページはトリガーが立ち、かつ auto-fill が大半を占める時に出力。組織判断・将来計画・運用系のページ(Quality Goals、Roadmap、Runbook、SLO、Postmortem、Security Posture)は **初回実行では出力されません** — 入力が揃ってから自分で書きます。
- **実シグナルから auto-fill。** マニフェストからコンテナ一覧、doc コメントからモジュール責務、エクスポートシンボルから glossary シード、CI 設定から contributing ルール、migrations から ER テーブル。確実に埋められないセルは **そのページ自体を出力しません** — オーナー名や SLO 数値のでっち上げは禁止。
- **`TODO(human):` は 1 ファイルにだけ** — ADR テンプレート(`decisions/0000-template.md`)。
- **冪等。** デフォルトはセクション単位の reconcile: auto-managed セクションは現在のシグナルから refresh、hand-edits は保持、ユーザー追加セクションは素通し。`--missing-only` で追加のみ、`--force` で emit セット内ページを再生成(手書き編集を上書き)。
- **frontmatter は最小** — 1 ページ 1 フィールド(`title:`)。

ページカタログは Diátaxis(目的)、arc42(アーキテクチャセクション)、C4 model(ズーム階層)、戦略的 DDD(Bounded Context)、ADR(意思決定記録)、SRE(運用ページ)、DeepWiki(AI Wiki の経験則)を統合しています。

トリガーフレーズ: 「scaffold the docs tree」、「generate the wiki skeleton」、「build the documentation from scratch」、「/heal-doc-scaffold」。

## `/heal-doc-review` — 監査スキル

読み取り専用。`heal status --json` を読み、`[features.docs]` スライスにフィルタリングし、次の 2 つを返します:

1. ドキュメントツリーの **アーキテクチャ的読解** — 支配的な軸は「Tutorial が実際のインストール手順からドリフト」「API リファレンスが古い」「コンセプトドキュメントのリンクが切れている」のどれか?
2. **優先順位付きドキュメント修正 TODO リスト** — Tutorial / How-to のドリフトを最初に(混乱した初心者ユーザーが最高レバレッジの修正対象)、次に Reference、最後に Explanation。

メトリクス別の **Diátaxis** レンズでのフレーミング:

| メトリクス | Diátaxis 観点 |
|---|---|
| `doc_freshness` | ユーザが最初に読むセクションが動いたか? |
| `doc_drift` | このドキュメントからコピペしたスニペットはまだコンパイルできるか? |
| `doc_coverage` | このソースはそもそもドキュメントを必要とするか? |
| `doc_link_health` | 内部ナビゲーションは動くか? |
| `orphan_pages` | このページは実際のエントリポイントから到達できるか? |
| `todo_density` | このドキュメントは活発に建設中か、静かに見捨てられているか? |

ソースは編集しません。レビューを読んで「これも直してほしい」と思ったら、その場で Claude Code に伝えれば対応に移れます(「リンク切れを直して」「インストール手順を書き直して」など)。機械的な破損は `/heal-doc-patch` を経由し、人間の判断が要る書き直し(「このセクションはまだ必要か?」「この how-to を tutorial と reference に分けるべきか?」)は自動適用されず、あなたの指示を待ちます。

### review と patch を分けている理由

**Patch** が引き受けるのは機械的なものです — 解決しないリンク、リネームされた識別子、誰からもリンクされていないページ。**Review** はそれに加えて、**人間の判断が必要な項目** を拾い上げます — ドリフトした tutorial を書き直すべきか、消すべきか、別ページに統合すべきか。reference のドリフトは本物のバグか、意図的に簡略化した例か。両者を 1 つのオート実行に混ぜると、判断のいる項目を勝手に書き換えるか、簡単な修正を放置するか、どちらかになります。

トリガーフレーズ: 「review the docs health」、「where should we fix documentation」、「/heal-doc-review」。

## `/heal-doc-patch` — 書き込みスキル

`.heal/findings/latest.json` の docs スライスを 1 件ずつ解消します。**1 修正 1 コミット**。

**事前チェック**(失敗すると起動拒否):

- クリーンな worktree。
- キャッシュ存在(欠けていれば `heal status --json` で埋める)。
- `[features.docs]` 有効(`.heal/config.toml`)。
- `doc_freshness` / `doc_drift` / `doc_coverage` の finding がスコープ内なら、`.heal/doc_pairs.json` が存在すること(無ければ `/heal-doc-pair-setup` に誘導)。

**メトリクス別の手筋**:

| メトリクス | デフォルトの手 |
|---|---|
| `doc_link_health`(`MissingPath`) | 相対パスを更新。リネームされていれば `git log --diff-filter=R` で追う。 |
| `doc_link_health`(`MissingAnchor`) | heading slug を合わせる。heading がリネームされていれば新 slug にリンクを更新。 |
| `doc_drift` | 古い識別子の参照を消す、または明確なリネームがあれば新名前で復活。 |
| `orphan_pages` | 親 README からのリンクを足す。削除すべきならエスカレート。 |
| `todo_density` | 解決可能な TODO を解決し、残りは GitHub issue へ。 |
| `doc_freshness` | ペアソースを再読み、影響を受けたセクションを書き直す。voice と構造は保持(内容同期で再設計ではない)。 |
| `doc_coverage` | ユーザにエスカレート。patch スキルが一方的に新規ドキュメントを書くことはしない(空スタブを避けるため)。 |

**Refusal**(スキル本体に encoded):

- **stub-without-content** — `doc_coverage` を黙らせるためだけの 1 行ファイルは書かない。
- **cosmetic-pass** — コミット単位の修正は特定の Finding を狙う。drive-by の文章書き直しや「ついでに」の reformatting はしない。
- **doc-as-truth-source** — ドリフトしたドキュメントに合わせてコードを更新することはしない。ソースが canonical、ドキュメントが更新される側。

**制約**: 1 finding = 1 commit、Conventional Commit subject + `Refs: F#<finding_id>` trailer、push / amend / `--no-verify` はしない。Code または Test ファミリのメトリクスに属する findings はスキップ。

トリガーフレーズ: 「fix the doc findings」、「drain the doc cache」、「patch stale docs」、「/heal-doc-patch」。
