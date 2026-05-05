---
title: Code · スキル
description: 常時オン Code ファミリ向け Claude Code スキル 4 種 — /heal-cli、/heal-setup、/heal-code-review、/heal-code-patch。
---

heal は Claude Code 向けスキルセットを同梱しているので、収集したメトリクスは Claude セッションへ自動的に流れます。リポジトリごとに 1 回インストールします:

```sh
heal skills install
```

このページは 4 つの Code ファミリスキルを扱います。doc 系は [Docs › スキル](/heal/ja/docs/skills/)、test 系は [Test › スキル](/heal/ja/test/skills/) を参照。

スキルセットは `heal` バイナリに同梱されているので、インストールされるバージョンは常にバイナリと一致します。`heal` をアップグレードしたら `heal skills update` で更新します。

## `/heal-code-review` — 監査スキル

読み取り専用。`heal status --all --json` を取り込み、フラグ付きコードを深く読み、次の 2 つを返します:

1. **アーキテクチャ的読解** — findings を _システムとして_ 何を語っているか(支配的な軸: complexity、duplication、coupling、hub)。
2. **優先順位付き TODO リスト** — デフォルトで T0 のみ。T1 は別の「帯域があれば」セクション、Advisory はカウントだけ表示。

ソースは編集しません。チームが「設計上のもので直さない」(意図的に複雑な税金エンジン、手続き的に凝集したパーサコンビネータなど)と判断した項目には `heal mark accept` も推奨できます。

レビューを読んで「これも直してほしい」と思ったら、その場で Claude Code に伝えれば対応に移れます(「最初の 3 件を直して」「Extract Function 系から片付けて」など)。機械的な修正は `/heal-code-patch` 経由に流れ、判断が要る項目は自動適用されず、あなたの指示を待ちます。

### review と patch を分けている理由

**Patch** が引き受けるのは機械的な修正です — 長い関数を Extract Function、重複したブロックを共有ヘルパへ、ドリフトしたテストをソースに合わせ直す。「どのファイルを触るか分かれば、手筋は明らか」というタイプ。

**Review** はそれに加えて、**人間の判断が必要な項目** も拾い上げます — このハブは分割すべきか? この重複は実は別概念が同じ形に育ったものでは? — チームの文脈なしでは正解が決まらない問いです。だからレビューは提案して止まる。両者を 1 つのオート実行に混ぜると、判断のいる項目を取りこぼすか、機械的な山を放置するか、どちらかになります。

トリガーフレーズ: 「review the codebase health」、「what does heal say?」、「where should we refactor?」、「/heal-code-review」。

## `/heal-code-patch` — 書き込みスキル

`.heal/findings/latest.json` を Severity 順に 1 件ずつ消化し、修正ごとに 1 コミット。ループは **T0(`must`)のみ** 解消します。T1 / Advisory は表示するだけで自動解消はしません。

**事前チェック**(失敗すると起動拒否):

- クリーンな worktree。
- キャッシュ存在(欠けていれば `heal status --json` で埋める)。
- Calibration の存在(無いとすべての Finding が `Severity::Ok` になり、対象がない)。

**メトリクス別の手筋**(Fowler / Tornhill 語彙):

| メトリクス | 主な手 |
|---|---|
| `ccn` / `cognitive` | Extract Function、Guard Clauses、Decompose Conditional |
| `duplication` | Extract Function / Method、Pull Up Method、Rule of Three |
| `change_coupling`(`.symmetric` 含む) | アーキテクチャの継ぎ目を可視化(coupling の自動修正は行わない) |
| `lcom` | クラスタ境界に沿って Extract Class |
| `hotspot` | Hotspot はフラグであって問題ではない。基底のメトリクスに対処 |

**制約**(スキルが強制): 1 finding = 1 commit、Conventional Commit subject + `Refs: F#<finding_id>` trailer、push / amend / `--no-verify` はしない。docs / test ファミリのメトリクスに属する findings はスキップ — そちらは `/heal-doc-patch` / `/heal-test-patch` の担当です。

トリガーフレーズ: 「fix the heal findings」、「drain the cache」、「work through the TODO list」、「/heal-code-patch」。

## `/heal-cli` — CLI リファレンス

`heal` CLI の簡潔で完全なリファレンス。各サブコマンド、各 `--json` の形、各コマンドが読み書きする `.heal/` ファイルを網羅しています。Claude が他のスキルから `heal` をシェル実行する前にこれを読み込むので、CLI 表面は安定した契約として扱われます。

## `/heal-setup` — セットアップウィザード

ワンショットのセットアップウィザード。プロジェクトを calibrate し、コードベースを見渡し、strictness レベル(Strict / Default / Lenient)を選んでもらって `.heal/config.toml` を書く / 更新したあと、`[features.docs]` / `[features.test]` の有効化を順に確認し、有効化する場合は対応するセットアップスキル(`/heal-doc-pair-setup` / `/heal-test-reporter-setup`)に連携します。

コードベースが大きく動いて基準を動かしたくなったとき、または Critical を持続的に解消し終えたときは再実行を — そういう局面では `heal calibrate --force` も推奨します。

## メンテナンス

```sh
heal skills update     # heal バイナリ更新後にリフレッシュ(ドリフト認識付き)
heal skills status     # ドリフトしたファイルを一覧
heal skills uninstall  # 同梱スキルをすべて削除
```

`update` は手編集されたファイルをそのまま残します(警告付き)。`--force` で上書き可。`uninstall` は `.claude/skills/heal-*` 配下をすべて削除しますが、あなたが書いた兄弟スキルは残り、`.heal/` 配下のプロジェクトデータも基本的に手付かずです。
