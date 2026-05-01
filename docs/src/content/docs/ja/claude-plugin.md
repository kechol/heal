---
title: Claude プラグイン
description: 同梱の Claude Code プラグインが、heal のメトリクスをどのように Claude セッションに繋ぐか — `/heal-code-review` 監査と `/heal-code-patch` 修復ループを含む。
---

heal には Claude Code 用のプラグインが同梱されています。これにより、
heal が収集するメトリクスを Claude セッションに自動で流し込めます。
プラグインは `heal skills install` でリポジトリごとに一度だけインス
トールします。それ以降:

- Claude のすべての編集とターン終了が `.heal/logs/` に記録されます。
- リードオンリースキル `/heal-code-review` が
  `.heal/checks/latest.json` を監査し、アーキテクチャレベルの所見と
  優先度付きのリファクタ TODO リストを返します。
- write スキル `/heal-code-patch` が同じキャッシュを Severity 順に
  1 コミット 1 Finding ずつ消化していきます。キャッシュが空になる
  か、セッションを止めるまで続きます。

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
    ├── heal-code-review/
    │   ├── SKILL.md
    │   └── references/
    │       ├── metrics.md
    │       └── architecture.md
    └── heal-code-patch/
        └── SKILL.md
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

修復ループは SessionStart ナッジではなく `heal-code-patch` スキル
（後述）を通じて動きます。

## 監査スキル: `/heal-code-review`

リードオンリー。`heal check --all --json` を取り込み、フラグ付きの
コードを深く読み込んで、2 つの成果物を返します:

1. **アーキテクチャ的な所見** — Finding を _リスト_ ではなく
   _システム_ として読み解いたもの（複雑度・重複・結合・ハブの
   いずれが支配的軸か）。
2. **優先度付き TODO リスト** — ファイル / 関数を特定した具体的な
   リファクタ提案。各エントリには確立されたリファクタパターン名と、
   期待されるメトリクスの動きが付与されます。

スキルは言語非依存で、テンプレートを押し付けるのではなく、コード
ベースに見て取れるスタイルに合わせて提案を調整します。リファレンス
ファイルが 2 つ同梱されており、必要なときだけ読み込まれます:

- `references/metrics.md` — 各メトリクス（`loc` / `ccn` /
  `cognitive` / `churn` / `change_coupling` / `duplication` /
  `hotspot` / `lcom`）が何を測っているか、背景の文献、しきい値、
  典型的な偽陽性。
- `references/architecture.md` — リファクタ提案で使う語彙集:
  モジュールの深さ（Ousterhout）、レイヤード / ヘキサゴナル
  アーキテクチャ（Cockburn、Evans）、DDD（Evans、Vernon）、
  および提案が満たすべき _コードベース尊重_ のルール。

`/heal-code-review` は提案のみで、ソースを変更しません。書き込み側は
`/heal-code-patch` です。

## write スキル: `/heal-code-patch`

`/heal-code-patch` は `/heal-code-review` の修復ループ対応版です。
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

メトリクスごとに `/heal-code-patch` は確立されたリファクタリング語彙
（Fowler、Tornhill）にマッピングされます。

| メトリクス                  | 主な手法                                                                               |
| --------------------------- | -------------------------------------------------------------------------------------- |
| `ccn` / `cognitive`         | Extract Function、Replace Nested Conditional with Guard Clauses、Decompose Conditional |
| `duplication`               | Extract Function / Method、Pull Up Method、Form Template Method、Rule of Three         |
| `change_coupling`           | アーキテクチャ的な seam を表面化 — `/heal-code-patch` は coupling を自動修正しない       |
| `change_coupling.symmetric` | 同様 — 強い「責務の混在」シグナルは人間の判断が必要                                    |
| `lcom`                      | クラスをクラスタ境界で分割 — 通常 Extract Class                                        |
| `hotspot`                   | Hotspot は問題ではなくフラグ；裏にある CCN/dup/coupling に対処する                     |

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
