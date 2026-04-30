---
title: CLI
description: heal のサブコマンドを日々の重要度順に並べた一覧と、運用で使うコマンドの例。
---

`heal` は単一のバイナリです。すべての操作は以下のサブコマンドのど
れかを通じて行います。引数の詳細は `heal --help` または
`heal <subcommand> --help` を参照してください。

## ユーザー向けコマンド

日々の重要度の高い順に並んでいます — 通常使うのは上位 3 つで、下に
行くほど調査・メンテナンス用です。

| コマンド         | 用途                                                                                            |
| ---------------- | ----------------------------------------------------------------------------------------------- |
| `heal init`      | カレントリポジトリに `.heal/` をセットアップし、calibrate して post-commit フックを設置。       |
| `heal skills`    | 同梱の Claude プラグインのインストール／更新／状態確認／削除。                                  |
| `heal check`     | `.heal/checks/latest.json` のキャッシュを描画する（`--refresh` で再スキャン上書き）。           |
| `heal status`    | メトリクスごとのサマリと、前スナップショットからの差分を表示。                                  |
| `heal calibrate` | コードベース相対の Severity しきい値を再 calibrate する。                                       |
| `heal logs`      | 生のフックイベントログ（`.heal/logs/`）を覗く。                                                 |
| `heal snapshots` | メトリクス／calibrate イベント（`.heal/snapshots/`）のタイムラインを覗く。                      |
| `heal checks`    | `.heal/checks/` 内の `CheckRecord` を新しい順に一覧表示。                                       |
| `heal fix`       | `.heal/checks/` のレコード／Finding 単位の操作 — `show <id>` / `diff` / `mark`。                |
| `heal compact`   | 古いイベントログをまとめて圧縮／削除する。冪等で、手動実行も安全。                              |

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
heal init                # 対話モード — Claude スキルのインストールを訊きます
heal init --yes          # Claude プラグインも抽出（プロンプトなし）
heal init --no-skills    # プラグインは入れない（CI や Claude を使わない場合）
heal init --force        # 既存の config.toml / フックを上書き
```

`heal init` の処理:

1. `.heal/` を作成し、`config.toml`、`calibration.toml`、
   `snapshots/`、`logs/`、`checks/` を配置。
2. 全オブザーバーを一度走らせ、メトリクスごとにコードベースのパー
   センタイル分布を計算 — これが `calibration.toml` になります
   （先頭には `heal calibrate --force` を案内するコメントヘッダ）。
3. `.git/hooks/post-commit` をインストール（冪等 — スクリプトには
   コメントの目印が付いており、再インストールでも行が重複しません）。
4. 最初の `MetricsSnapshot` を `.heal/snapshots/` に追記。Severity
   の集計値も含まれます。
5. `claude` コマンドが `PATH` にあれば、同梱の Claude プラグインを
   `.claude/plugins/heal/` に展開するか確認します。プロンプトのデ
   フォルトは `Y`。プロンプトを飛ばして必ずインストールするには
   `--yes`、確認なしで飛ばすには `--no-skills`。`claude` が `PATH`
   に **無い** 場合は、確認なしで黙ってスキップします（プラグインを
   入れても話す相手がいないため）。

完了時には "Installed:" サマリで、生成したすべてのファイルを一覧表
示します — config、calibration、初回 snapshot、post-commit フック、
そして Claude プラグインのパス（またはスキップ理由）。

再実行は安全です。`--force` を付けない限り `config.toml` はそのま
ま残ります。post-commit フックは heal の目印を持つときのみ置き換え
られます。heal 由来でない `post-commit` フックがすでに存在する場合、
`heal init` はそれを残します — 上書きするには `--force`。

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

- 1 つのリードオンリースキル `heal-code-check` — `heal check --all
  --json` を取り込み、フラグ付きコードを深く読み込み、アーキテクチャ
  的な所見と優先度付きのリファクタ TODO リストを返します（リファレ
  ンスは `skills/heal-code-check/references/` 以下）。
- 1 つの write スキル `heal-code-fix` — `.heal/checks/latest.json`
  を Severity 順（`Critical 🔥` 先頭）に 1 コミット 1 Finding ずつ
  消化。

スキルの仕様詳細は[Claude プラグイン](/heal/ja/claude-plugin/)を参
照。

## `heal check`

各オブザーバーを実行し、`calibration.toml` で Finding を Severity
分類して、`.heal/checks/latest.json` に書き出します（`/heal-code-check`
が監査し、`/heal-code-fix` が消化する TODO リスト）。

```sh
heal check                              # キャッシュを再描画（デフォルト）
heal check --refresh                    # 再スキャンして latest.json を上書き
heal check --metric lcom                # LCOM の Finding のみ
heal check --severity critical          # Critical のみ（`--all` で以上を含む）
heal check --feature src/payments       # 特定パスプレフィックスに絞る
heal check --all                        # Medium / Ok と低 Severity の Hotspot セクションを表示
heal check --top 5                      # 各 Severity バケットを 5 行で打ち切り
heal check --json                       # CheckRecord 形式を stdout へ
```

デフォルトの `heal check` は `.heal/checks/latest.json` を読み出す
だけのリードオンリー描画なので、キャッシュが温まっていれば実質コ
スト 0 です。`--refresh` を指定した場合のみキャッシュを破棄して
再スキャン・上書きします（このパスだけが書き込みを行います）。
`heal init` 直後など、キャッシュが存在しない場合は最初の実行で自
動的にスキャンするので、フラグなしでも問題なく動きます。

出力は Finding を `🔴 Critical 🔥 / 🔴 Critical / 🟠 High 🔥 /
🟠 High / 🟡 Medium / ✅ Ok` の下にグループ化し（最後の 2 つは
`--all` が必要）、ファイル単位に 1 行へ集約します。最後に
`Goal: 0 Critical, 0 High` と、`claude /heal-code-fix` を指す "next
steps" の行が続きます。`--all` 指定時は、「Severity は低いが上位
10% の Hotspot」に該当するファイルを別セクション（`Ok / Medium 🔥`）
で追加表示します。

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

## `heal calibrate`

```sh
heal calibrate            # calibration.toml が無ければ作成、あればドリフト・チェックのみ
heal calibrate --force    # 常に再スキャンして calibration.toml を上書き
```

heal は **絶対に** 自動で recalibrate しません。

- `.heal/calibration.toml` が **無い** とき、`heal calibrate` は再
  スキャンしてファイルを書き出します。（通常は `heal init` がブー
  トストラップ時に作成するので、このパスは init 前のみ。）
- ファイルが **ある** ときのデフォルト実行は読み取り専用です。ド
  リフト・トリガー（90 日経過、コードベースファイル数 ±20%、30
  日連続で Critical が 0）を評価して推奨を表示し、再生成のヒント
  として `--force` を案内します。

post-commit ナッジは同じトリガーで "consider recalibrating" の 1
行ヒントを先頭に付けます。実行するかは常にユーザー判断です
（`heal calibrate --force`）。

生成された `calibration.toml` の先頭には、ファイルの来歴と再生成
コマンドを示すコメントヘッダが付きます。docs を見なくてもファイル
を開いた人が辿れるようにしてあります。`floor_critical` のオーバー
ライドは `config.toml` 側に置いてください（`heal calibrate --force`
で消えないようにするため）。

calibration の監査記録は `.heal/snapshots/` に
`event = "calibrate"` レコードとして残ります — `heal logs` でコ
ミットと並んで表示されます。

## `heal logs` / `heal snapshots` / `heal checks`

`.heal/` 配下の append-only ストアを覗くための 3 つの並列ブラウザ
です。`--since` / `--limit` / `--json` は共通。`heal logs` と
`heal snapshots` は `--filter <event>` も受け付けます。

```sh
heal logs --filter commit --limit 10        # フックイベント: commit / edit / stop
heal logs --since 2026-04-01T00:00:00Z

heal snapshots --filter calibrate            # MetricsSnapshot + calibrate
heal snapshots --json --limit 5

heal checks                                  # 新しい順の CheckRecord サマリ
heal checks --json --limit 20                # {check_id, started_at, head_sha, severity_counts, …} の JSON
```

| ソース                | 中身                                                                          | コマンド         |
| --------------------- | ----------------------------------------------------------------------------- | ---------------- |
| `.heal/logs/`         | `commit` / `edit` / `stop` フックイベント（軽量メタデータのみ）。             | `heal logs`      |
| `.heal/snapshots/`    | `commit`（`MetricsSnapshot`）と `calibrate`（`CalibrationEvent`）のタイムライン。 | `heal snapshots` |
| `.heal/checks/`       | `heal check` が書き出した `CheckRecord` 履歴。                                | `heal checks`    |

`heal status` は snapshots を統合したビュー、`heal snapshots` は raw
タイムラインです。v0.2 以前の `session-start` イベントは
SessionStart ナッジとともに廃止されました。

## `heal fix`

`.heal/checks/` 上のレコード単位／Finding 単位の操作です。閲覧は
`heal checks` 側、`heal fix` は「TODO リストを処理している」とい
う意図の動詞です。

```sh
heal fix show <check_id>              # 単一レコードを描画
heal fix show <check_id> --json       # 安定した形（`heal check --json` と同じ）

heal fix diff                         # 最新キャッシュ vs ライブスキャン
heal fix diff <from>                  # <from> vs ライブスキャン
heal fix diff <from> <to>             # 2 件のキャッシュ間（スキャンなし）
heal fix diff --all --json            # Improved/Unchanged も表示 + JSON

heal fix mark --finding-id <id> --commit-sha <sha>   # /heal-code-fix が呼ぶ
```

引数のセマンティクスは `git diff` と同じ。`<to>` を省略すると「ワー
キングツリーの現状を即席スキャンしたもの」が右辺になる（保存はさ
れない）。`<from>` も省略すると最新キャッシュレコードが左辺。
`vs live` モードの diff の後、FROM レコードの全 Finding が
`fixed.jsonl` にマークされていれば、`heal check --refresh` で
fixed.jsonl ↔ regressed.jsonl を整合するよう促す 1 行ヒントが出ま
す。

`.heal/checks/<segment>.jsonl` と `latest.json` の writer は
`heal check` のみ。`heal fix mark` がもう一つの writer で、
`fixed.jsonl` に `FixedFinding` を 1 行追記するだけ。
`heal fix show` / `heal fix diff` と `heal checks` は完全リードオンリー。

## `heal compact`

```sh
heal compact            # 90 日超を gzip、365 日超を削除し、結果サマリを 1 行表示
heal compact --verbose  # 触ったファイルを 1 行ずつ表示
```

`.heal/{snapshots,logs,checks}/` を辿り、保持ポリシーを適用します。

- **90 日**を超えたセグメントは その場で gzip 圧縮されます
  （`YYYY-MM.jsonl` → `YYYY-MM.jsonl.gz`）。リーダーは両方の形式
  を透過的に扱います。
- **365 日**を超えたセグメントは削除されます。

同じ処理は `heal hook commit` の中でも自動実行されるので、手動実
行は基本的に診断や 1 回限りのクリーンアップ用途です（バックアップ
からの復旧後、長く静かなリポジトリのまとめて圧縮、など）。冪等な
ので、すでに圧縮済みのディレクトリで再実行しても何もしません。

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
- **`heal fix diff`**（引数なし）はセッション中の進捗確認に便利
  です。最新キャッシュとライブスキャンを比較するだけで、
  `.heal/checks/` には余計なレコードを書きません。
- **post-commit フックは保持する。** 削除すると新しいスナップショッ
  トが記録されなくなり、`heal status` / `heal checks` は前回までの
  差分を表示し続けます。
