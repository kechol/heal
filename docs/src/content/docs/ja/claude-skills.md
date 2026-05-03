---
title: Claude スキル
description: heal が同梱する Claude Code スキル群が、heal のメトリクスをどのように Claude セッションに繋ぐか — `/heal-code-review` 監査と `/heal-code-patch` 修復ループ、加えて `/heal-cli` と `/heal-config` のヘルパースキル。
---

heal には Claude Code 向けのスキルセットが同梱されています。これにより、heal が収集するメトリクスを Claude セッションに自動で流し込めます。`heal skills install` でリポジトリごとに一度だけインストールします。それ以降:

- リードオンリースキル `/heal-code-review` が `.heal/findings/latest.json` を監査し、アーキテクチャレベルの所見と優先度付きのリファクタ TODO リストを返します。
- write スキル `/heal-code-patch` が同じキャッシュを Severity 順に 1 コミット 1 Finding ずつ消化していきます。キャッシュが空になるか、セッションを止めるまで続きます。
- ヘルパースキル `/heal-cli` と `/heal-config` が、CLI 駆動と `config.toml` チューニングのリファレンスを Claude に渡します。

## インストール

```sh
heal skills install
```

これで各スキルが直接 `<project>/.claude/skills/` 配下に展開されます。Claude Code はプロジェクトスコープのスキルをここからネイティブに発見します。

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

スキルセットはコンパイル時に `heal` バイナリに埋め込まれているため、インストールされるバージョンは常にバイナリと一致します。`heal` をアップグレードした後は `heal skills update` でリフレッシュしてください。

## 監査スキル: `/heal-code-review`

リードオンリー。`heal status --all --json` を取り込み、フラグ付きのコードを深く読み込んで、2 つの成果物を返します。

1. **アーキテクチャ的な所見** — Finding を _リスト_ ではなく _システム_ として読み解いたもの（複雑度・重複・結合・ハブのいずれが支配的軸か）。
2. **優先度付き TODO リスト** — デフォルトでは **T0 (`must`) のみ** を対象とします。T1 (`should`) は別セクション「If bandwidth permits」として、Advisory はカウントのみで surface します。TODO エントリはファイル / 関数を特定した具体的なリファクタ提案で、各エントリには確立されたリファクタパターン名と期待されるメトリクスの動きが付与されます。

スキルは言語非依存で、テンプレートを押し付けるのではなく、コードベースに見て取れるスタイルに合わせて提案を調整します。リファレンスファイルが 3 つ同梱されており、必要なときだけ読み込まれます。

- `references/metrics.md` — 各メトリクス（`loc` / `ccn` / `cognitive` / `churn` / `change_coupling` / `duplication` / `hotspot` / `lcom`）が何を測っているか、背景の文献、しきい値、典型的な偽陽性。
- `references/architecture.md` — リファクタ提案で使う語彙集: モジュールの深さ（Ousterhout）、レイヤード / ヘキサゴナルアーキテクチャ（Cockburn、Evans）、DDD（Evans、Vernon）、リファクタパターンのレバレッジ階層、トラップカタログ、および提案が満たすべき _コードベース尊重_ のルール。
- `references/readability.md` — 提案の _positive_ 基準: ゴール階層（readability → maintainability → metric）、可読性の原則（Boswell、Ousterhout、Beck、Knuth）、5 つの判断質問テスト。

`/heal-code-review` は提案のみで、ソースを変更しません。書き込み側は `/heal-code-patch` です。

## write スキル: `/heal-code-patch`

`/heal-code-patch` は `/heal-code-review` の修復ループ対応版です。`.heal/findings/latest.json` を Severity 順に 1 件ずつ消化し、修正ごとに 1 コミットを切ります。

事前条件（満たされなければ起動を拒否）:

1. **クリーンな worktree。** worktree が dirty だとキャッシュの `worktree_clean = false` で、記録された数値はディスク上のソースと一致しません。スキルは停止し、コミットか stash を求めます。
2. **キャッシュが存在する。** `latest.json` がなければ、スキルは `heal status --json` を一度実行して populate します。
3. **calibration が存在する。** `calibration.toml` がなければ、すべての Finding が `Severity::Ok` になり、対象がありません。

ループは **T0 (`must`) のみ** を drain します — T1 / Advisory は review 用に surface しますが自動 drain しません。T0 が空になったら T1 に黙って延長せず、セッションを終了します。

```
キャッシュの T0 に Finding がある間:
    次の 1 件を選ぶ（T0 内で Severity 🔥 desc）
    対象ファイルを読み、メトリクスに対する最小の修正を計画
    変更を適用
    テスト / 型検査 / linter を可能な範囲で実行
    git add ...; git commit -m "<conventional message + Refs: F#<id>>"
    heal mark fix --finding-id <id> --commit-sha <sha>
    heal status --refresh --json   # 再スキャン; fixed.json ↔ regressed.jsonl を整合
    Finding が regress していたら今回はそのまま、次へ
    そうでなければ続行
```

停止条件: T0 が空、ユーザーが中断（Ctrl+C / Stop）、またはスキルが人間の判断（アーキテクチャ判断、ビジネスルール）を必要とする Finding に当たったとき。最後のケースでは、トレードオフを提示して適用前に確認します。T0 が空で T1 / Advisory が残っている場合、スキルはサマリを表示し、提案レベルでの議論のために `/heal-code-review` の実行を勧めます。

メトリクスごとに `/heal-code-patch` は確立されたリファクタリング語彙（Fowler、Tornhill）にマッピングされます。

| メトリクス                  | 主な手法                                                                               |
| --------------------------- | -------------------------------------------------------------------------------------- |
| `ccn` / `cognitive`         | Extract Function、Replace Nested Conditional with Guard Clauses、Decompose Conditional |
| `duplication`               | Extract Function / Method、Pull Up Method、Form Template Method、Rule of Three         |
| `change_coupling`           | アーキテクチャ的な seam を表面化 — `/heal-code-patch` は coupling を自動修正しない     |
| `change_coupling.symmetric` | 同様 — 強い「責務の混在」シグナルは人間の判断が必要                                    |
| `lcom`                      | クラスをクラスタ境界で分割 — 通常 Extract Class                                        |
| `hotspot`                   | Hotspot は問題ではなくフラグ；裏にある CCN/dup/coupling に対処する                     |

スキルが強制する制約:

- 1 Finding = 1 コミット。Finding をまたいで squash しない。
- Conventional Commit の subject + body + `Refs: F#<finding_id>` trailer。
- push しない、amend しない、`--no-verify` しない。
- キャッシュを超えてループを延長しない。新しい Finding を扱いたい場合は新しい `heal status` 実行に渡す。

## ヘルパースキル: `/heal-cli` と `/heal-config`

ループには関わらない 2 つのスキルが、手続きではなく直接的なリファレンスを Claude に渡す目的で同梱されています。

`/heal-cli` は `heal` CLI の簡潔かつ完全なリファレンスです — すべてのサブコマンド、すべての `--json` 形状、そして各コマンドが読み書きする `.heal/` 内ファイルを網羅します。Claude は別のスキルから `heal` を実行する前にこれを読み込むので、CLI 表面は `--help` から推測するのではなく安定した契約として扱われます。

`/heal-config` はプロジェクトを calibrate し、コードベースを調査し、strictness レベル（Strict / Default / Lenient）を選んでもらった上で `.heal/config.toml` を作成または更新します。`references/config.md` には `config.toml` の全キーの完全なスキーマと、strictness ごとのレシピ表が載っています。最初のセットアップ、コードベースの構造変化（vendor ツリー追加、レイヤ書き換え）の後、あるいは品質バーをしきい値を覚えなおさずにシフトしたいときに使います。

`/heal-config` は calibration ベースラインのドリフトを検知して `heal calibrate --force` を推奨します — ファイル数が大きく動いた、calibration がプロジェクト速度に対して古い、Critical が長く 0 のままなど。チェックは冪等です — 介在する変更がなければ何度実行しても同じ推奨が返ります。

## スキルの更新

`heal` バイナリをアップグレードした後:

```sh
heal skills update
```

**マニフェスト不要のドリフト検出**。インストール後の各 `SKILL.md` は YAML frontmatter に `metadata:` ブロック（`heal-version`、`heal-source`）が刻印されます。`update` はディスク上のバイト列からドリフトを直接導出します。`metadata:` ブロックを取り除いた canonical バイト列を、同梱の生バイト列と比較します。

- canonical（メタデータ除去後）が同梱と一致するファイルは、新しい同梱バージョンで上書きされます。
- メタデータブロック以外に手動編集があるファイルは警告とともに残されます。
- `--force` を渡すと手動編集も含めてすべて上書きします。

`heal skills status` はドリフトしたファイルを並べて比較表示します。同じ on-disk + 同梱バイト比較がどのマシンでも実行されるので、別のチームメイトが再インストールしても同じ判定になります — 共有するサイドカーマニフェストは不要です。

## 削除

```sh
heal skills uninstall
```

`.claude/skills/heal-*` 配下の同梱スキルディレクトリを削除します。ユーザが置いたほかのスキルディレクトリは残り、`.heal/` 配下のプロジェクトデータも触られません。

## なぜ同梱なのか

`cargo install heal-cli` という単一の配信チャネルが、CLI と対応スキルを同時に提供します。バージョンを揃えてリリースすることで、スキルとバイナリのバージョンミスマッチを防ぎます。トレードオフは、スキルセットが `heal` バイナリと同じ鮮度であるという点です。スキルプロンプトを独立に書き換えたい場合は `.claude/skills/heal-*/` を手で編集してください — `heal skills update` 時にそれらがドリフトとしてマークされる前提で。
