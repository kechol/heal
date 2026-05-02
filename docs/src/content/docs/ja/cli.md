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

| コマンド         | 用途                                                                                                  |
| ---------------- | ----------------------------------------------------------------------------------------------------- |
| `heal init`      | カレントリポジトリに `.heal/` をセットアップし、calibrate して post-commit フックを設置。             |
| `heal skills`    | 同梱の Claude スキルセットのインストール／更新／状態確認／削除。                                      |
| `heal status`    | `.heal/checks/latest.json` のキャッシュを描画する（`--refresh` で再スキャン上書き）。「いまの TODO」。 |
| `heal diff`      | ライブ worktree とキャッシュ済 `CheckRecord` の差分（デフォルト ref: `HEAD`）。`git diff` 風。       |
| `heal metrics`   | メトリクスごとのサマリと、前スナップショットからの差分を表示。                                        |
| `heal calibrate` | コードベース相対の Severity しきい値を再 calibrate する。                                             |
| `heal logs`      | 生のフックイベントログ（`.heal/logs/`）を覗く。                                                       |
| `heal snapshots` | メトリクス／calibrate イベント（`.heal/snapshots/`）のタイムラインを覗く。                            |
| `heal checks`    | `.heal/checks/` 内の `CheckRecord` を新しい順に一覧表示。                                             |
| `heal compact`   | 古いイベントログをまとめて圧縮／削除する。冪等で、手動実行も安全。                                    |

## 自動化向けコマンド

git の post-commit フックや Claude Code の `settings.json` フック
コマンド、`/heal-code-patch` スキルから自動的に呼ばれます。通常は
手動で叩きません。`--help` には表示されません。

| コマンド          | 呼び出し元                                              | 用途                                                                                      |
| ----------------- | ------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `heal hook`       | git post-commit + Claude `PostToolUse` / `Stop`         | オブザーバー実行、スナップショット／イベントログ書き出し、Severity ナッジ。               |
| `heal mark-fixed` | `/heal-code-patch` スキル                               | コミットで Finding を直したことを `.heal/checks/fixed.jsonl` に記録。                     |

---

## `heal init`

git リポジトリ内で heal をブートストラップします。

```sh
heal init                # 対話モード — Claude スキルのインストールを訊きます
heal init --yes          # Claude スキルも抽出（プロンプトなし）
heal init --no-skills    # スキルは入れない（CI や Claude を使わない場合）
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
5. `claude` コマンドが `PATH` にあれば、同梱のスキルセットを
   `.claude/skills/` に展開し、HEAL のフックコマンドを
   `.claude/settings.json` にマージするか確認します。プロンプトのデ
   フォルトは `Y`。プロンプトを飛ばして必ずインストールするには
   `--yes`、確認なしで飛ばすには `--no-skills`。`claude` が `PATH`
   に **無い** 場合は、確認なしで黙ってスキップします（スキルを入れ
   ても話す相手がいないため）。

完了時には "Installed:" サマリで、生成したすべてのファイルを一覧表
示します — config、calibration、初回 snapshot、post-commit フック、
そして Claude スキルのパス（またはスキップ理由）。

再実行は安全です。`--force` を付けない限り `config.toml` はそのま
ま残ります。post-commit フックは heal の目印を持つときのみ置き換え
られます。heal 由来でない `post-commit` フックがすでに存在する場合、
`heal init` はそれを残します — 上書きするには `--force`。

## `heal skills`

`.claude/skills/` 配下の同梱スキルセットと、
`.claude/settings.json` 内の HEAL フックコマンドを管理します。

```sh
heal skills install     # スキル展開 + フックコマンドのマージ（リポジトリごとに一度）
heal skills update      # heal バイナリを更新した後にリフレッシュ
heal skills status      # インストール済みと同梱版を比較
heal skills uninstall   # スキル、マニフェスト、HEAL のフックコマンドを削除
```

スキルセットはコンパイル時に `heal` バイナリに埋め込まれているため、
`heal skills install` は使用中のバイナリと一致するバージョンを必ず
展開します。`update` はドリフトを検知し、手で編集されたファイルは
そのまま残します（`--force` で上書き可）。

同梱されるスキルは 4 つ:

- `/heal-code-review`（read-only）— `heal status --all --json` を取
  り込み、フラグ付きコードを深く読み込み、アーキテクチャ的な所見と
  優先度付きのリファクタ TODO リストを返します（リファレンスは
  `.claude/skills/heal-code-review/references/` 以下）。
- `/heal-code-patch`（write）— `.heal/checks/latest.json` を
  Severity 順（`Critical 🔥` 先頭）に 1 コミット 1 Finding ずつ消化。
- `/heal-cli` — `heal` CLI 表面の簡潔なリファレンス。
- `/heal-config` — calibrate して strictness レベルを訊き、
  `config.toml` を書き出す。

`uninstall` はマーケットプレイス時代のレガシーレイアウト（旧
`.claude/plugins/heal/`、`.claude-plugin/marketplace.json`、および
`heal@heal-local` 系の settings エントリ）も同時に掃除するので、
古い heal からの移行は uninstall + 再インストールで完結します。

スキルの仕様詳細は[Claude スキル](/heal/ja/claude-skills/)を参照。

## `heal status`

各オブザーバーを実行し、`calibration.toml` で Finding を Severity
分類して、`.heal/checks/latest.json` に書き出します（`/heal-code-review`
が監査し、`/heal-code-patch` が消化する TODO リスト）。

```sh
heal status                              # キャッシュを再描画（デフォルト）
heal status --refresh                    # 再スキャンして latest.json を上書き
heal status --metric lcom                # LCOM の Finding のみ
heal status --severity critical          # Critical のみ（`--all` で以上を含む）
heal status --feature src/payments       # 特定パスプレフィックスに絞る
heal status --all                        # Medium / Ok と低 Severity の Hotspot セクションを表示
heal status --top 5                      # 各 Severity バケットを 5 行で打ち切り
heal status --json                       # CheckRecord 形式を stdout へ
```

デフォルトの `heal status` は `.heal/checks/latest.json` を読み出す
だけのリードオンリー描画なので、キャッシュが温まっていれば実質コ
スト 0 です。`--refresh` を指定した場合のみキャッシュを破棄して
再スキャン・上書きします（このパスだけが書き込みを行います）。
`heal init` 直後など、キャッシュが存在しない場合は最初の実行で自
動的にスキャンするので、フラグなしでも問題なく動きます。

出力は Finding を `🔴 Critical 🔥 / 🔴 Critical / 🟠 High 🔥 /
🟠 High / 🟡 Medium / ✅ Ok` の下にグループ化し（最後の 2 つは
`--all` が必要）、ファイル単位に 1 行へ集約します。最後に
`Goal: 0 Critical, 0 High` と、`claude /heal-code-patch` を指す "next
steps" の行が続きます。`--all` 指定時は、「Severity は低いが上位
10% の Hotspot」に該当するファイルを別セクション（`Ok / Medium 🔥`）
で追加表示します。

## `heal metrics`

```sh
heal metrics
heal metrics --json
heal metrics --metric complexity
heal metrics --metric lcom
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
を開いた人が辿れるようにしてあります。`floor_critical` / `floor_ok`
のオーバーライドは `config.toml` 側に置いてください（`heal calibrate
--force` で消えないようにするため）。

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

| ソース             | 中身                                                                              | コマンド         |
| ------------------ | --------------------------------------------------------------------------------- | ---------------- |
| `.heal/logs/`      | `commit` / `edit` / `stop` フックイベント（軽量メタデータのみ）。                 | `heal logs`      |
| `.heal/snapshots/` | `commit`（`MetricsSnapshot`）と `calibrate`（`CalibrationEvent`）のタイムライン。 | `heal snapshots` |
| `.heal/checks/`    | `heal status` が書き出した `CheckRecord` 履歴。                                    | `heal checks`    |

`heal metrics` は snapshots を統合したビュー、`heal snapshots` は raw
タイムラインです。v0.2 以前の `session-start` イベントは
SessionStart ナッジとともに廃止されました。

## `heal diff`

ライブ worktree とキャッシュ済 `CheckRecord`（`head_sha` が解決した
git ref と一致するもの）の bucket diff を出します。デフォルトの ref
は `HEAD` ——「いまの worktree は最後のコミットと比べてどう動いた
か?」:

```sh
heal diff                              # ライブ vs HEAD のキャッシュ
heal diff main                         # ライブ vs main のキャッシュ
heal diff v0.2.1                       # ライブ vs v0.2.1 タグ
heal diff HEAD~5                       # ライブ vs 5 コミット前
heal diff --all                        # Improved + Unchanged も表示
heal diff --json                       # 安定した JSON 形
```

`<git-ref>` には `git rev-parse` で解釈できるものを渡せます。対応
する `CheckRecord` がキャッシュにない場合（その ref で
`heal status` を一度も走らせていない等）はエラー + ヒント表示。
コミット + `heal status` を走らせるか、ref を checkout して
`heal status --refresh` してください。

右辺は **常にワーキングツリーの即席スキャン** で、永続化されません。
出力 bucket は Resolved / Regressed / Improved / New / Unchanged ＋
進捗パーセンテージです。

`.heal/checks/<segment>.jsonl` と `latest.json` の writer は
`heal status` のみ。`heal mark-fixed` がもう一つの writer で、
`fixed.jsonl` に `FixedFinding` を 1 行追記するだけ。`heal diff`
と `heal checks` は完全リードオンリー。

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

このコマンドは git の post-commit フックと Claude Code の
`settings.json` フックコマンドから自動的に呼ばれます。デバッグのた
めに手動実行が役立つことがあります。

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

- **`heal status` が標準ワークフローです。** 意味のあるコミットの後
  に実行して、キャッシュをリフレッシュし TODO リストの残りを確認し
  ます。
- **`heal diff`**（引数なし）はセッション中の進捗確認に便利です。
  HEAD のキャッシュとライブスキャンを比較するだけで、`.heal/checks/`
  には余計なレコードを書きません。
- **post-commit フックは保持する。** 削除すると新しいスナップショッ
  トが記録されなくなり、`heal metrics` / `heal checks` は前回までの
  差分を表示し続けます。
