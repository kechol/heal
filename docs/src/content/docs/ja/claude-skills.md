---
title: Claude スキル
description: heal が同梱する Claude Code スキル群が、heal のメトリクスをどのように Claude セッションに繋ぐか — `/heal-code-review` 監査と `/heal-code-patch` 修復ループ、加えて `/heal-cli` と `/heal-config` のヘルパースキル。
---

heal には Claude Code 向けのスキルセットが同梱されています。これに
より、heal が収集するメトリクスを Claude セッションに自動で流し込
めます。`heal skills install` でリポジトリごとに一度だけインストー
ルします。それ以降:

- Claude のすべての編集とターン終了が `settings.json` に直書きされ
  たフック経由で `.heal/logs/` に記録されます（シェルスクリプトラッ
  パーは介在しません）。
- リードオンリースキル `/heal-code-review` が
  `.heal/checks/latest.json` を監査し、アーキテクチャレベルの所見と
  優先度付きのリファクタ TODO リストを返します。
- write スキル `/heal-code-patch` が同じキャッシュを Severity 順に
  1 コミット 1 Finding ずつ消化していきます。キャッシュが空になる
  か、セッションを止めるまで続きます。
- ヘルパースキル `/heal-cli` と `/heal-config` が、CLI 駆動と
  `config.toml` チューニングのリファレンスを Claude に渡します。

v0.2 以前の SessionStart ナッジは廃止されました。同じ役割は
post-commit フック（`heal init` の git インストールが設置）がより
シンプルなセマンティクスで担います —
[アーキテクチャ › 全体像](/heal/ja/architecture/#全体像)を参照。

## インストール

```sh
heal skills install
```

これで各スキルが直接 `<project>/.claude/skills/` 配下に展開されます。
Claude Code はプロジェクトスコープのスキルをここからネイティブに発
見します:

```
.claude/skills/
├── heal-cli/
│   └── SKILL.md
├── heal-code-patch/
│   └── SKILL.md
├── heal-code-review/
│   ├── SKILL.md
│   └── references/
│       ├── architecture.md
│       ├── metrics.md
│       └── readability.md
└── heal-config/
    ├── SKILL.md
    └── references/
        └── config.md
```

スキルセットはコンパイル時に `heal` バイナリに埋め込まれているた
め、インストールされるバージョンは常にバイナリと一致します。
`heal` をアップグレードした後は `heal skills update` でリフレッシュ
してください。

## フックの組み込み方

`heal skills install` は同時に
`<project>/.claude/settings.json` にも 2 つのエントリをマージしま
す:

```jsonc
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit|Write|MultiEdit",
        "hooks": [{ "type": "command", "command": "heal hook edit" }]
      }
    ],
    "Stop": [
      { "hooks": [{ "type": "command", "command": "heal hook stop" }] }
    ]
  }
}
```

どちらも同じ `heal hook` エントリポイントを呼び戻します:

| フックイベント | 振る舞い                                                                              |
| -------------- | ------------------------------------------------------------------------------------- |
| `PostToolUse`  | Claude による Edit / Write / MultiEdit を `.heal/logs/`（イベントのみ）に記録します。 |
| `Stop`         | Claude のターン終了をログに記録します。                                               |

どちらも純粋なログ記録です — オブザーバーを動かさないので、Claude
のターンに測定可能なレイテンシは追加しません。

マージは **加算的** です: ユーザーの既存フックは正確な `command`
一致による dedupe で温存され、`heal skills uninstall` は HEAL 自身
の command 行のみを削除します。あなたが書いたほかのエントリはその
まま残ります。

`heal hook` 自体も、`.heal/` がないプロジェクトで呼び出されたとき
は静かに no-op します。HEAL を導入していないリポジトリで Claude
セッションを動かしても、勝手に HEAL 状態を書き起こすことはありませ
ん。Edit / Stop は内部失敗を握りつぶすので、インライン command が
Claude のループをブロックすることもありません。`commit`（git フッ
クから呼び出される）は元のエラー伝播パスを保ちます。

修復ループは SessionStart ナッジではなく `/heal-code-patch` スキル
（後述）を通じて動きます。

## 監査スキル: `/heal-code-review`

リードオンリー。`heal status --all --json` を取り込み、フラグ付きの
コードを深く読み込んで、2 つの成果物を返します:

1. **アーキテクチャ的な所見** — Finding を _リスト_ ではなく
   _システム_ として読み解いたもの（複雑度・重複・結合・ハブの
   いずれが支配的軸か）。
2. **優先度付き TODO リスト** — デフォルトでは **T0 (`must`) のみ** を対
   象とします。T1 (`should`) は別セクション「If bandwidth permits」
   として、Advisory はカウントのみで surface します。TODO エントリ
   はファイル / 関数を特定した具体的なリファクタ提案で、各エントリ
   には確立されたリファクタパターン名と期待されるメトリクスの動き
   が付与されます。

スキルは言語非依存で、テンプレートを押し付けるのではなく、コード
ベースに見て取れるスタイルに合わせて提案を調整します。リファレンス
ファイルが 3 つ同梱されており、必要なときだけ読み込まれます:

- `references/metrics.md` — 各メトリクス（`loc` / `ccn` /
  `cognitive` / `churn` / `change_coupling` / `duplication` /
  `hotspot` / `lcom`）が何を測っているか、背景の文献、しきい値、
  典型的な偽陽性。
- `references/architecture.md` — リファクタ提案で使う語彙集:
  モジュールの深さ（Ousterhout）、レイヤード / ヘキサゴナル
  アーキテクチャ（Cockburn、Evans）、DDD（Evans、Vernon）、
  リファクタパターンのレバレッジ階層、トラップカタログ、および提案
  が満たすべき _コードベース尊重_ のルール。
- `references/readability.md` — 提案の *positive* 基準: ゴール階
  層（readability → maintainability → metric）、可読性の原則
  （Boswell、Ousterhout、Beck、Knuth）、5 つの判断質問テスト。

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
   `heal status --json` を一度実行して populate します。
3. **calibration が存在する。** `calibration.toml` がなければ、
   すべての Finding が `Severity::Ok` になり、対象がありません。

ループは **T0 (`must`) のみ** を drain します — T1 / Advisory は
review 用に surface しますが自動 drain しません。T0 が空になったら
T1 に黙って延長せず、セッションを終了します。

```
キャッシュの T0 に Finding がある間:
    次の 1 件を選ぶ（T0 内で Severity 🔥 desc）
    対象ファイルを読み、メトリクスに対する最小の修正を計画
    変更を適用
    テスト / 型検査 / linter を可能な範囲で実行
    git add ...; git commit -m "<conventional message + Refs: F#<id>>"
    heal mark-fixed --finding-id <id> --commit-sha <sha>
    heal status --refresh --json   # 再スキャン; fixed.jsonl ↔ regressed.jsonl を整合
    Finding が regress していたら今回はそのまま、次へ
    そうでなければ続行
```

停止条件: T0 が空、ユーザーが中断（Ctrl+C / Stop）、またはスキルが
人間の判断（アーキテクチャ判断、ビジネスルール）を必要とする Finding
に当たったとき。最後のケースでは、トレードオフを提示して適用前に確
認します。T0 が空で T1 / Advisory が残っている場合、スキルはサマリ
を表示し、提案レベルでの議論のために `/heal-code-review` の実行を勧
めます。

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
  場合は新しい `heal status` 実行に渡す。

## ヘルパースキル: `/heal-cli` と `/heal-config`

ループには関わらない 2 つのスキルが、手続きではなく直接的なリファ
レンスを Claude に渡す目的で同梱されています。

`/heal-cli` は `heal` CLI の簡潔かつ完全なリファレンスです — すべ
てのサブコマンド、すべての `--json` 形状、そして各コマンドが読み書
きする `.heal/` 内ファイルを網羅します。Claude は別のスキルから
`heal` を実行する前にこれを読み込むので、CLI 表面は `--help` から
推測するのではなく安定した契約として扱われます。

`/heal-config` はプロジェクトを calibrate し、コードベースを調査し、
strictness レベル（Strict / Default / Lenient）を選んでもらった上
で `.heal/config.toml` を作成または更新します。
`references/config.md` には `config.toml` の全キーの完全なスキーマ
と、strictness ごとのレシピ表が載っています。最初のセットアップ、
コードベースの構造変化（vendor ツリー追加、レイヤ書き換え）の後、
あるいは品質バーをしきい値を覚えなおさずにシフトしたいときに使いま
す。

## スキルの更新

`heal` バイナリをアップグレードした後:

```sh
heal skills update
```

**ドリフトを意識**します。heal はインストールしたファイルそれぞれ
のフィンガープリントを `.heal/skills-install.json` に記録します。
更新時:

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

以下を削除します:

- マニフェストに記録されている `.claude/skills/heal-*` 配下のスキル
  ディレクトリ。
- `.heal/skills-install.json`。
- `.claude/settings.json` 内の HEAL 自身の command エントリ。それ
   以外のユーザーフックはそのまま残し、エントリが他に何もなければ
  ファイルごと削除されます。
- 古いバージョンの heal がマーケットプレイス経由で配布していた頃の
  **レガシーレイアウト**: 旧 `.claude/plugins/heal/` ツリー、
  `.claude-plugin/marketplace.json`、および `settings.json` 内の
  `extraKnownMarketplaces["heal-local"]` /
  `enabledPlugins["heal@heal-local"]` エントリ。

`.heal/` 配下のプロジェクトデータは触られません。

マーケットプレイス経由で配布していた古い heal からアップグレードす
るときの安全な移行手順は、`heal skills uninstall` を一度走らせて
から `heal skills install` です。（`install` と `update` は意図的
に旧レイアウトを移行しません。新しいバイナリを旧レイアウトと共存
させたままにすると、uninstall するまでフックが二重発火します。）

## なぜ同梱なのか

`cargo install heal-cli` という単一の配信チャネルが、CLI と対応ス
キルを同時に提供します。バージョンを揃えてリリースすることで、ス
キルとバイナリのバージョンミスマッチを防ぎます。トレードオフは、ス
キルセットが `heal` バイナリと同じ鮮度であるという点です。スキル
プロンプトを独立に書き換えたい場合は `.claude/skills/heal-*/` を手
で編集してください — `heal skills update` 時にそれらがドリフトと
してマークされる前提で。
