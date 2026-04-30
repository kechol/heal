---
title: Claude プラグイン
description: 同梱の Claude Code プラグインが、heal のメトリクスをどのように Claude セッションに繋ぐか — `/heal-fix` 修復ループを含む。
---

heal には Claude Code 用のプラグインが同梱されています。これにより、
heal が収集するメトリクスを Claude セッションに自動で流し込めます。
プラグインは `heal skills install` でリポジトリごとに一度だけインス
トールします。それ以降:

- Claude のすべての編集とターン終了が `.heal/logs/` に記録されます。
- 5 つのリードオンリー `check-*` スキルが利用可能になり、特定メト
  リクスについて Claude にオンデマンドで尋ねられます。
- write スキル `/heal-fix` が `.heal/checks/latest.json` を Severity
  順に 1 コミット 1 Finding ずつ消化していきます。キャッシュが空に
  なるか、セッションを止めるまで続きます。

v0.2 以前の SessionStart ナッジは廃止されました。同じ役割は
post-commit フック（`heal init` の git インストールが設置）がより
シンプルなセマンティクスで担います —
[アーキテクチャ › 全体像](/heal/ja/architecture/#全体像)を参照。

## インストール

```sh
heal skills install
```

これでプラグインツリーが `.claude/plugins/heal/` に展開されます。

```
.claude/plugins/heal/
├── plugin.json
├── hooks/
│   ├── claude-post-tool-use.sh
│   └── claude-stop.sh
└── skills/
    ├── check-overview/SKILL.md
    ├── check-hotspots/SKILL.md
    ├── check-complexity/SKILL.md
    ├── check-duplication/SKILL.md
    ├── check-coupling/SKILL.md
    └── heal-fix/SKILL.md
```

プラグインツリーはコンパイル時に `heal` バイナリに埋め込まれている
ため、インストールされるバージョンは常にバイナリと一致します。
`heal` をアップグレードした後は `heal skills update` でリフレッシュ
してください。

## フックがすること

プラグインには 2 つのフックが同梱されており、どちらも同じ
`heal hook` エントリポイントに戻って呼び出します。

| フックイベント | 振る舞い                                                                              |
| -------------- | ------------------------------------------------------------------------------------- |
| `PostToolUse`  | Claude による Edit / Write / MultiEdit を `.heal/logs/`（イベントのみ）に記録します。 |
| `Stop`         | Claude のターン終了をログに記録します。                                               |

どちらも純粋なログ記録です — オブザーバーを動かさないので、Claude
のターンに測定可能なレイテンシは追加しません。

修復ループは SessionStart ナッジではなく `heal-fix` スキル（後述）
を通じて動きます。

## 5 つの `check-*` スキル

`heal status --metric <X>` 呼び出しをラップし、結果の数値をプロジェ
クトの `response_language` で解説するリードオンリースキルです。

| スキル              | 機能                                                                   |
| ------------------- | ---------------------------------------------------------------------- |
| `check-overview`    | 有効な全メトリクスを 1 つの状況レポートに統合。                        |
| `check-hotspots`    | Hotspot ランキングに踏み込み、各上位ファイルがなぜスコアしたかを解説。 |
| `check-complexity`  | worst CCN / Cognitive 関数を辿り、リファクタ候補を提示。               |
| `check-duplication` | 重複ブロックをレビューし、ヘルパーを抽出できそうな箇所を提案。         |
| `check-coupling`    | 共変ペアをレビューし、抽象が欠けていそうな箇所を提案。                 |

インタラクティブな Claude セッション内で、5 つのうち任意のスキル
（`check-overview` / `check-hotspots` など）を名前で指定して使うよ
う Claude に依頼します。すべてリードオンリーです — `heal status`
や `heal check` を呼ぶことはできますが、ソースファイルは変更しま
せん。

## write スキル: `/heal-fix`

`/heal-fix` は `check-*` スキルの修復ループ対応版です。
`.heal/checks/latest.json` を Severity 順に 1 件ずつ消化し、修正ご
とに 1 コミットを切ります。

事前条件（満たされなければ起動を拒否）:

1. **クリーンな worktree。** worktree が dirty だとキャッシュの
   `worktree_clean = false` で、記録された数値はディスク上のソース
   と一致しません。スキルは停止し、コミットか stash を求めます。
2. **キャッシュが存在する。** `latest.json` がなければ、スキルは
   `heal check --json` を一度実行して populate します。
3. **calibration が存在する。** `calibration.toml` がなければ、
   すべての Finding が `Severity::Ok` になり、対象がありません。

ループ:

```
キャッシュに non-Ok Finding がある間:
    次の 1 件を選ぶ（Severity 順: Critical🔥 → Critical → High🔥 → High → Medium）
    対象ファイルを読み、メトリクスに対する最小の修正を計画
    変更を適用
    テスト / 型検査 / linter を可能な範囲で実行
    git add ...; git commit -m "<conventional message + Refs: F#<id>>"
    heal fix mark --finding-id <id> --commit-sha <sha>
    heal check --refresh --json   # 再スキャン; fixed.jsonl ↔ regressed.jsonl を整合
    Finding が regress していたら今回はそのまま、次へ
    そうでなければ続行
```

停止条件: キャッシュが空、ユーザーが中断（Ctrl+C / Stop）、または
スキルが人間の判断（アーキテクチャ判断、ビジネスルール）を必要とす
る Finding に当たったとき。最後のケースでは、トレードオフを提示し
て適用前に確認します。

メトリクスごとに `/heal-fix` は確立されたリファクタリング語彙
（Fowler、Tornhill）にマッピングされます。

| メトリクス                  | 主な手法                                                                                |
| --------------------------- | --------------------------------------------------------------------------------------- |
| `ccn` / `cognitive`         | Extract Function、Replace Nested Conditional with Guard Clauses、Decompose Conditional  |
| `duplication`               | Extract Function / Method、Pull Up Method、Form Template Method、Rule of Three          |
| `change_coupling`           | アーキテクチャ的な seam を表面化 — `/heal-fix` は coupling を自動修正しない             |
| `change_coupling.symmetric` | 同様 — 強い「責務の混在」シグナルは人間の判断が必要                                     |
| `lcom`                      | クラスをクラスタ境界で分割 — 通常 Extract Class                                         |
| `hotspot`                   | Hotspot は問題ではなくフラグ；裏にある CCN/dup/coupling に対処する                      |

スキルが強制する制約:

- 1 Finding = 1 コミット。Finding をまたいで squash しない。
- Conventional Commit の subject + body + `Refs: F#<finding_id>`
  trailer。
- push しない、amend しない、`--no-verify` しない。
- キャッシュを超えてループを延長しない。新しい Finding を扱いたい
  場合は新しい `heal check` 実行に渡す。

## プラグインの更新

`heal` バイナリをアップグレードした後:

```sh
heal skills update
```

**ドリフトを意識**します。heal はインストールしたファイルそれぞれ
のフィンガープリントを `.claude/plugins/heal/.heal-install.json` に
記録します。更新時:

- 記録された同梱フィンガープリントと一致するファイルは、新しい同梱
  バージョンで上書きされます。
- フィンガープリントが異なるファイル（手動編集されたもの）は警告と
  ともに残されます。
- `--force` を渡すと手動編集も含めてすべて上書きします。

`heal skills status` は、ドリフトしたファイルを並べて比較表示します。

## 削除

```sh
heal skills uninstall
```

`.claude/plugins/heal/` を削除します。それ以外は触りません。`.heal/`
配下のプロジェクトデータはそのまま残ります。

## なぜ同梱なのか

`cargo install heal-cli` という単一の配信チャネルが、CLI と対応プ
ラグインを同時に提供します。バージョンを揃えてリリースすることで、
プラグインとバイナリのバージョンミスマッチを防ぎます。トレードオフ
は、プラグインが `heal` バイナリと同じ鮮度であるという点です。スキ
ルプロンプトを独立に書き換えたい場合は `.claude/plugins/heal/` を
手で編集してください — `heal skills update` 時にそれらがドリフトと
してマークされる前提で。
