---
title: CLI
description: heal のサブコマンド一覧と、日々の運用で使うコマンドの例。
---

`heal` は単一のバイナリです。すべての操作は以下のサブコマンドのど
れかを通じて行います。引数の詳細は `heal --help` または
`heal <subcommand> --help` を参照してください。

## ユーザー向けコマンド

ターミナルから直接実行するコマンドです。

| コマンド      | 用途                                                                  |
| ------------- | --------------------------------------------------------------------- |
| `heal init`   | カレントリポジトリに `.heal/` と post-commit フックをセットアップ。   |
| `heal status` | 直近のメトリクスと前コミットからの差分を表示。                        |
| `heal logs`   | イベントログ（コミット、Claude 編集、セッション開始）を流し読みする。 |
| `heal check`  | Claude Code を呼び出し、メトリクスを読み解説してもらう。              |
| `heal skills` | 同梱の Claude プラグインのインストール、更新、削除。                  |

## 自動化向けコマンド

git の post-commit フックや Claude プラグインから自動的に呼ばれる
コマンドです。通常は手動で叩きません。

| コマンド    | 呼び出し元               | 用途                                                       |
| ----------- | ------------------------ | ---------------------------------------------------------- |
| `heal hook` | git と Claude プラグイン | オブザーバーを実行、スナップショットを書く、ナッジを出す。 |

---

## `heal init`

git リポジトリ内で heal をブートストラップします。

```sh
heal init
```

このコマンドは `.heal/`（`config.toml`、`snapshots/`、`logs/` を含
む）を作成し、`.git/hooks/post-commit` をインストールし、初回スナッ
プショットを取ります。再実行しても安全です。`--force` を付けない限
り設定ファイルはそのまま残り、post-commit フックにはコメントの目印
が付くため再インストールでも行が重複することはありません。

`post-commit` フックがすでに存在する場合、`heal init` はそれを上書
きしません。既存のフックを置き換えるには `--force` を渡します。

## `heal status`

主要なステータスコマンドです。

```sh
heal status
heal status --json
heal status --metric complexity
```

有効化されている各メトリクスのサマリ — プライマリ言語、worst-N の
複雑な関数、トップホットスポットなど — と、前コミットからの差分ブ
ロックを表示します。

`--metric <name>` で出力を単一のメトリクスに絞り込めます。指定でき
る名前: `loc`, `complexity`, `churn`, `change-coupling`,
`duplication`, `hotspot`。`--json` は同じデータを機械可読な JSON
で出力するため、`jq` でのパイプ処理に向きます。

`.heal/snapshots/` が空の場合（たとえば `heal init` 直後で初回コミッ
ト前）、コマンドはスナップショットがない旨を表示します。

## `heal logs`

`.heal/logs/` 以下のイベントログを読みます。

```sh
heal logs
heal logs --filter commit --limit 10
heal logs --since 2026-04-01T00:00:00Z
heal logs --json
```

各レコードは 1 行の JSON です。生成されるイベントは 5 種類です。

- `init` — `heal init` が一度だけ書き出します
- `commit` — git の post-commit フックが書き出します
- `edit` — Claude がファイルを編集したときに（プラグイン経由で）書き出されます
- `stop` — Claude のターンが終了したときに書き出されます
- `session-start` — Claude セッションが開いたときに書き出されます

`heal status` は `snapshots/`（重いメトリクスペイロード）を読み、
`heal logs` は `logs/`（軽量なイベントタイムライン）を読みます。
両者は補完関係にあります。

## `heal check`

直近のメトリクスを Claude Code にリードオンリーのプロンプトと一緒
に渡します。

```sh
heal check                    # デフォルト: 全メトリクスの俯瞰
heal check hotspots           # ホットスポットランキング
heal check complexity         # CCN と Cognitive のウォークスルー
heal check duplication
heal check coupling
```

各バリアントは `claude -p`（Claude のヘッドレスモード）を、対応する
メトリクスにフォーカスする小さな `check-*` スキルとともに起動しま
す。スキルファイルは同梱プラグインの一部です — 詳しくは
[Claude プラグイン](/heal/ja/claude-plugin/) を参照。

`--` 以降の引数はそのまま `claude` に渡されます。

```sh
heal check overview -- --model sonnet --effort medium
```

`--model`、`--effort`、`--no-cache` などのフラグを渡すのに便利です。

`heal check` は `heal status` の解説版にあたるコマンドであり、ソー
スファイルを変更しません。

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
を必ず展開します。`update` はドリフトを検知します。手で編集された
ファイルはそのまま残します（`--force` で上書き可）。

---

## `heal hook`（自動化）

このコマンドは git の post-commit フックと Claude プラグインのフッ
クから自動的に呼ばれます。通常は手動で実行しませんが、デバッグの際
には手動実行が役に立つことがあります。

```sh
heal hook commit          # post-commit: オブザーバー実行、スナップショット書き出し
heal hook edit            # Claude PostToolUse: ファイル編集をログに記録
heal hook stop            # Claude Stop: セッション終了をログに記録
heal hook session-start   # Claude SessionStart: しきい値ナッジを出す
```

たとえば、空の JSON ペイロードでターミナルから `heal hook
session-start` を実行すると、現在のスナップショット差分でどのルー
ルが発火するかを、実際の Claude セッションを開かずに確認できます。

---

## ヒント

- **意味のあるコミットの後には `heal status` を実行する。** 高速で、
  Claude セッションを開く前のサニティチェックになります。
- **`heal check` は `heal status` を散文化したもの。** 数値の解釈が
  必要なときに使います。check スキルは `heal status --metric <X>`
  をフォーカスされたプロンプトでラップしています。
- **post-commit フックは保持する。** 削除すると新しいスナップショッ
  トが記録されなくなり、`heal status` は前回までの差分を表示し続け
  ます。
