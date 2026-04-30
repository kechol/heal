---
title: CLI
description: heal のサブコマンド一覧と、日々の運用で使うコマンドの例。
---

`heal` は単一のバイナリです。すべての操作は以下のサブコマンドのど
れかを通じて行います。引数の詳細は `heal --help` または
`heal <subcommand> --help` を参照してください。

## ユーザー向けコマンド

ターミナルから直接実行するコマンドです。

| コマンド         | 用途                                                                                       |
| ---------------- | ------------------------------------------------------------------------------------------ |
| `heal init`      | カレントリポジトリに `.heal/` をセットアップし、calibrate して post-commit フックを設置。  |
| `heal status`    | メトリクスごとのサマリと、前スナップショットからの差分を表示。                             |
| `heal logs`      | 生のフックイベントログを流し読みする。                                                     |
| `heal check`     | 全オブザーバーを実行し、Severity で分類し、`.heal/checks/latest.json` を更新する。         |
| `heal cache`     | `.heal/checks/` キャッシュのリードオンリービュー（`log` / `show` / `diff`）と `mark-fixed`。|
| `heal calibrate` | コードベース相対の Severity しきい値を再 calibrate する。                                  |
| `heal skills`    | 同梱の Claude プラグインのインストール、更新、削除。                                       |

## 自動化向けコマンド

git の post-commit フックや Claude プラグインから自動的に呼ばれる
コマンドです。通常は手動で叩きません。

| コマンド    | 呼び出し元               | 用途                                                                       |
| ----------- | ------------------------ | -------------------------------------------------------------------------- |
| `heal hook` | git と Claude プラグイン | オブザーバー実行、スナップショット書き出し、post-commit Severity ナッジ。  |

---

## `heal init`

git リポジトリ内で heal をブートストラップします。

```sh
heal init
```

`heal init` の処理:

1. `.heal/` を作成し、`config.toml`、`calibration.toml`、
   `snapshots/`、`logs/`、`checks/` を配置。
2. 全オブザーバーを一度走らせ、メトリクスごとにコードベースのパー
   センタイル分布を計算 — これが `calibration.toml` になります。
3. `.git/hooks/post-commit` をインストール（冪等 — スクリプトには
   コメントの目印が付いており、再インストールでも行が重複しません）。
4. 最初の `MetricsSnapshot` を `.heal/snapshots/` に追記。Severity
   の集計値も含まれます。

再実行は安全です。`--force` を付けない限り `config.toml` はそのま
ま残ります。post-commit フックは heal の目印を持つときのみ置き換え
られます。heal 由来でない `post-commit` フックがすでに存在する場合、
`heal init` はそれを残します — 上書きするには `--force`。

## `heal status`

```sh
heal status
heal status --json
heal status --metric complexity
heal status --metric lcom
```

有効化されている各メトリクスのサマリ — プライマリ言語、worst-N の
複雑な関数、トップ Hotspot、最も分割可能なクラスなど — と、前コミッ
トからの差分ブロックを表示します。`--metric <name>` で出力を単一の
オブザーバーに絞り込めます。指定できる名前: `loc`、`complexity`、
`churn`、`change-coupling`、`duplication`、`hotspot`、`lcom`。
`--json` は同じデータを機械可読な JSON で出力するため、`jq` でのパ
イプ処理に向きます。

`.heal/snapshots/` が空の場合（たとえば `heal init` 直後で初回コミッ
ト前）、コマンドはスナップショットがない旨を表示します。

## `heal logs`

```sh
heal logs
heal logs --filter commit --limit 10
heal logs --since 2026-04-01T00:00:00Z
heal logs --json
```

各レコードは 1 行の JSON です。`.heal/logs/` には 3 種類のイベント
が書き出されます。

- `commit` — git の post-commit フックが書き出します（sha、parent、
  author、メッセージサマリ、ファイル/行数）。
- `edit` — Claude がファイルを編集したとき（PostToolUse フック）。
- `stop` — Claude のターンが終了したとき（Stop フック）。

`heal status` は `snapshots/`（重いメトリクスペイロード）を読み、
`heal logs` は `logs/`（軽量なイベントタイムライン）を読みます。
両者は補完関係にあります。v0.2 以前の `session-start` イベントは、
SessionStart ナッジとともに廃止されました。

## `heal check`

全オブザーバーを実行し、`calibration.toml` を使って各 Finding を
Severity で分類し、結果を `.heal/checks/latest.json` に書き出しま
す（`/heal-fix` が読む TODO リスト）。

```sh
heal check                              # Severity ごとの全体ビュー
heal check --metric lcom                # LCOM の Finding のみ
heal check --severity critical          # Critical のみ（`--all` で以上を含む）
heal check --feature src/payments       # 特定パスプレフィックスに絞る
heal check --hotspot                    # 低 Severity の Hotspot ファイルを表示
heal check --top 5                      # 各 Severity バケットを 5 行で打ち切り
heal check --json                       # CheckRecord 形式を stdout へ
heal check --since-cache                # 再スキャンせず最新キャッシュを再描画
```

出力は Finding を `🔴 Critical 🔥 / 🔴 Critical / 🟠 High 🔥 /
🟠 High / 🟡 Medium / ✅ Ok` の下にグループ化し（最後の 2 つは
`--all` が必要）、ファイル単位に 1 行へ集約します。最後に
`Goal: 0 Critical, 0 High` と、`claude /heal-fix` を指す "next steps"
の行が続きます。

キャッシュヒット時は重いスキャンをショートサーキットします。同じ
`head_sha` + `config_hash` + クリーンな worktree ならば、オブザー
バーを再実行せずに直前のレコードを返します。

v0.2 以前の位置引数（`overview` / `hotspots` / `complexity` /
`duplication` / `coupling`）は今も deprecation alias として動作し
ます — 警告を出して対応する `--metric` / `--hotspot` フラグに翻訳
されます。v0.3 で削除予定です。

## `heal cache`

`.heal/checks/` のリードオンリーな調査と、`/heal-fix` が使う唯一の
mutation サブコマンド（`mark-fixed`）です。

```sh
heal cache log                          # 新しい順に CheckRecord 一覧
heal cache log --json --limit 20

heal cache show <check_id>              # 単一レコードを描画
heal cache show <check_id> --json       # 安定した形

heal cache diff                         # 直近 2 件のレコード
heal cache diff <from> <to>             # 明示ペア
heal cache diff --worktree              # ライブツリー vs 最新キャッシュ、書き込みなし
heal cache diff --all --json            # Improved/Unchanged も表示 + JSON

heal cache mark-fixed --finding-id <id> --commit-sha <sha>
```

`.heal/checks/` への唯一の writer は `heal check` です。`heal cache *`
は `mark-fixed` を除き状態を変更しません。`mark-fixed` は
`fixed.jsonl` に `FixedFinding` 1 行を追記するだけです。

## `heal calibrate`

```sh
heal calibrate                          # 再スキャンして新しい calibration.toml を書く
heal calibrate --reason "annual review" # 監査ログにタグ付け
heal calibrate --check                  # 自動検知トリガーを評価のみ、書き込みなし
```

heal は **絶対に** 自動で recalibrate しません。post-commit ナッジ
は、`heal calibrate --check` が発火する状況（90 日経過、コードベー
スファイル数が ±20%、30 日連続で Critical が 0）で 1 行のヒント
"consider recalibrating" を先頭に付けます。実行するかは常にユーザー
判断です。

calibration の監査記録は `.heal/snapshots/` に
`event = "calibrate"` レコードとして残ります — `heal logs` でコ
ミットと並んで表示されます。

## `heal skills`

`.claude/plugins/heal/` 以下の同梱 Claude プラグインを管理します。

```sh
heal skills install     # プラグインを展開（リポジトリごとに一度）
heal skills update      # heal バイナリを更新した後にリフレッシュ
heal skills status      # インストール済みと同梱版を比較
heal skills uninstall   # .claude/plugins/heal/ を削除
```

プラグインツリーはコンパイル時に `heal` バイナリに埋め込まれている
ため、`heal skills install` は使用中のバイナリと一致するバージョン
を必ず展開します。`update` はドリフトを検知し、手で編集されたファ
イルはそのまま残します（`--force` で上書き可）。

同梱プラグインに含まれるもの:

- 5 つのリードオンリー `check-*` スキル（`overview` / `hotspots` /
  `complexity` / `duplication` / `coupling`） — `heal status --metric <x>`
  をラップ。
- 1 つの write スキル `heal-fix` — `.heal/checks/latest.json` を
  Severity 順（`Critical 🔥` 先頭）に 1 コミット 1 Finding ずつ消化。

---

## `heal hook`（自動化）

このコマンドは git の post-commit フックと Claude プラグインのフッ
クから自動的に呼ばれます。デバッグのために手動実行が役立つことがあ
ります。

```sh
heal hook commit          # post-commit: オブザーバー実行、スナップショット書き、ナッジ表示
heal hook edit            # Claude PostToolUse: ファイル編集をログに記録
heal hook stop            # Claude Stop: ターン終了をログに記録
```

post-commit ナッジは、`Critical` と `High` の Finding をすべて
stdout に出します（`Medium` と `Ok` は静かなまま）。Hotspot フラグ
付きのエントリが先頭に来ます。クールダウンはありません — 同じ問題
は修正されるまで毎コミット出続けます。それが狙いです。

---

## ヒント

- **`heal check` が標準ワークフローです。** 意味のあるコミットの後
  に実行して、キャッシュをリフレッシュし TODO リストの残りを確認し
  ます。
- **`heal cache diff --worktree`** はセッション中の進捗確認に便利
  です。`.heal/checks/` に余計なレコードを追加しません。
- **post-commit フックは保持する。** 削除すると新しいスナップショッ
  トが記録されなくなり、`heal status` / `heal cache log` は前回まで
  の差分を表示し続けます。
