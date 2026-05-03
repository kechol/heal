---
title: CLI
description: heal のサブコマンドを日々の重要度順に並べた一覧と、運用で使うコマンドの例。
---

`heal` は単一のバイナリです。すべての操作は以下のサブコマンドのどれかを通じて行います。引数の詳細は `heal --help` または `heal <subcommand> --help` を参照してください。

## ユーザー向けコマンド

日々使う操作は実質この 4 つです。

| コマンド      | 用途                                                                                                          |
| ------------- | ------------------------------------------------------------------------------------------------------------- |
| `heal init`   | カレントリポジトリに `.heal/` をセットアップし、calibrate して post-commit フックを設置。                     |
| `heal skills` | 同梱の Claude スキルセットのインストール／更新／状態確認／削除。                                              |
| `heal status` | 現在の TODO リストを表示する（`--refresh` で再スキャン）。`.heal/findings/` を読みます。                      |
| `heal diff`   | ライブ worktree と過去のコミットを比較する（デフォルト ref: `HEAD`）。findings の `git diff` 版。             |

## 自動化向けコマンド

ユーザーの代わりに走るコマンドです — git の post-commit フックから、同梱の Claude スキルから、あるいはコードベースが十分に変化したときにのみ。`--help` には表示されません。

| コマンド          | 駆動元                       | 用途                                                                                       |
| ----------------- | ---------------------------- | ------------------------------------------------------------------------------------------ |
| `heal hook`       | git post-commit              | コミットごとにオブザーバーを実行し Severity ナッジを表示。                                 |
| `heal mark-fixed` | `/heal-code-patch` スキル    | 1 コミット 1 finding の修正を記録して、次の `heal status` で整合させる。                   |
| `heal metrics`    | `/heal-code-review` スキル   | 各メトリクスのサマリを毎回ワーキングツリーから再計算。                                     |
| `heal calibrate`  | `/heal-config` スキル        | Severity しきい値を現在のコードベース分布にリセット。                                      |

`heal metrics` と `heal calibrate` をここに置いているのは、*いつ* 走らせるかを同梱スキルが判断するからです。`/heal-code-review` は監査の途中で各メトリクスのサマリを参照し、`/heal-config` は calibration ドリフトを監視して必要なら recalibrate を提案します。手動で叩くのは、Claude を介さずに生の出力が欲しいときだけです。

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

1. `.heal/` を作成し、`config.toml`、`calibration.toml`、`findings/`、そして `findings/` を除外する `.gitignore` を配置（`config.toml` と `calibration.toml` は git で追跡され、チームで同じ Severity ラダーを共有できます）。
2. 全オブザーバーを一度走らせ、メトリクスごとにコードベースのパーセンタイル分布を計算 — これが `calibration.toml` になります。
3. `.git/hooks/post-commit` をインストール（冪等 — 再インストールでも行が重複しません）。
4. `claude` コマンドが `PATH` にあれば、同梱のスキルセットを `.claude/skills/` に展開するか確認します。プロンプトのデフォルトは `Y`。プロンプトを飛ばして必ずインストールするには `--yes`、確認なしで飛ばすには `--no-skills`。`claude` が `PATH` に **無い** 場合は、確認なしで黙ってスキップします（スキルを入れても話す相手がいないため）。

完了時には "Installed:" サマリで、生成したすべてのファイルを一覧表示します — config、calibration、post-commit フック、そして Claude スキルのパス（またはスキップ理由）。

再実行は安全です。`--force` を付けない限り `config.toml` はそのまま残ります。post-commit フックは heal の目印を持つときのみ置き換えられます。heal 由来でない `post-commit` フックがすでに存在する場合、`heal init` はそれを残します — 上書きするには `--force`。

## `heal skills`

`.claude/skills/` 配下の同梱スキルセットを管理します。

```sh
heal skills install     # スキル展開（リポジトリごとに一度）
heal skills update      # heal バイナリを更新した後にリフレッシュ
heal skills status      # インストール済みと同梱版を比較
heal skills uninstall   # スキルを削除
```

スキルセットはコンパイル時に `heal` バイナリに埋め込まれているため、`heal skills install` は使用中のバイナリと一致するバージョンを必ず展開します。`update` はドリフトを検知し、手で編集されたファイルはそのまま残します（`--force` で上書き可）。

同梱されるスキルは 4 つ:

- `/heal-code-review`（read-only）— `heal status --all --json` を取り込み、フラグ付きコードを深く読み込み、アーキテクチャ的な所見と優先度付きのリファクタ TODO リストを返します。
- `/heal-code-patch`（write）— TODO リストを Severity 順（`Critical 🔥` 先頭）に 1 コミット 1 finding ずつ消化。
- `/heal-cli` — `heal` CLI 表面の簡潔なリファレンス。
- `/heal-config` — calibrate して strictness レベルを訊き、`config.toml` を書き出す。あわせて calibration ドリフトを検知して `heal calibrate --force` を提案します。

スキルの仕様詳細は [Claude スキル](/heal/ja/claude-skills/) を参照。

## `heal status`

各オブザーバーを実行し、Finding を Severity 分類して、`/heal-code-review` が監査し `/heal-code-patch` が消化する TODO リストを書き出します。

```sh
heal status                              # キャッシュを再描画（デフォルト）
heal status --refresh                    # 再スキャンしてキャッシュを上書き
heal status --metric lcom                # LCOM の Finding のみ
heal status --severity critical          # Critical のみ（`--all` で以上を含む）
heal status --feature src/payments       # 特定パスプレフィックスに絞る
heal status --all                        # Medium / Ok と低 Severity の Hotspot セクションを表示
heal status --top 5                      # 各 Severity バケットを 5 行で打ち切り
heal status --json                       # 機械可読な形式を stdout へ
```

デフォルトの `heal status` はキャッシュ済 TODO のリードオンリー描画なので、キャッシュが温まっていれば実質コスト 0 です。`--refresh` を指定した場合のみキャッシュを破棄して再スキャン・上書きします（このパスだけが書き込みを行います）。`heal init` 直後などキャッシュが存在しない場合は最初の実行で自動的にスキャンするので、フラグなしでも問題なく動きます。

出力は Finding を `🔴 Critical 🔥 / 🔴 Critical / 🟠 High 🔥 / 🟠 High / 🟡 Medium / ✅ Ok` の下にグループ化し（最後の 2 つは `--all` が必要）、ファイル単位に 1 行へ集約します。最後に `Goal: 0 Critical, 0 High` と `claude /heal-code-patch` を指す "next steps" の行が続きます。`--all` 指定時は、「Severity は低いが上位 10% の Hotspot」に該当するファイルを別セクション（`Ok / Medium 🔥`）で追加表示します。

## `heal diff`

ライブ worktree と、指定した過去コミットでの finding を比較します。デフォルト ref は `HEAD` ——「いまの worktree は最後のコミットと比べてどう動いたか?」:

```sh
heal diff                              # ライブ vs HEAD
heal diff main                         # ライブ vs main
heal diff v0.2.1                       # ライブ vs v0.2.1 タグ
heal diff HEAD~5                       # ライブ vs 5 コミット前
heal diff --all                        # Improved + Unchanged も表示
heal diff --json                       # 機械可読な形式
```

`<git-ref>` には `git rev-parse` で解釈できるものを渡せます。heal は対象 ref を **現在の** `config.toml` / `calibration.toml` で再評価するので、apples-to-apples の比較になります — 当時の評価ではなく、いまのルールで then-and-now を見ているわけです。

出力 bucket は Resolved / Regressed / Improved / New / Unchanged ＋ 進捗パーセンテージです。右辺は **常にワーキングツリーの即席スキャン** で、永続化されません。

非常に大きなリポジトリではこの比較が高コストになります。`config.toml` の `[diff]` で LOC 上限を設定でき、超過時は手動 2 ブランチ手順に切り替わります。詳しくは [設定 › `[diff]`](/heal/ja/configuration/#diff) を参照。

## `heal metrics`

```sh
heal metrics
heal metrics --json
heal metrics --metric complexity
heal metrics --metric lcom
```

有効化されている各メトリクスのサマリ — プライマリ言語、worst-N の複雑な関数、トップ Hotspot、最も分割可能なクラスなど — を表示します。`--metric <name>` で出力を単一のオブザーバーに絞り込めます。指定できる名前: `loc`、`complexity`、`churn`、`change-coupling`、`duplication`、`hotspot`、`lcom`。`--json` は同じデータを機械可読な JSON で出力するため、`jq` でのパイプ処理に向きます。

呼び出しごとにすべてをワーキングツリーから再計算します — 履歴を保持しないため、過去との差分は出ません。

## `heal calibrate`

```sh
heal calibrate            # calibration.toml が無ければ作成、あれば no-op
heal calibrate --force    # 常に再スキャンして calibration.toml を上書き
```

heal は **絶対に** 自動で recalibrate しません — コードベースを実際に改善するリファクタが、暗黙のうちにゴールポストを動かしてしまっては困るからです。`--force` を実行するのは次の場面です。

- 大きな構造変更で分布が変わったとき（`/heal-config` スキルがそれを監視して提案します）。
- `config.toml` の `floor_critical` / `floor_ok` を変えて、パーセンタイルラダーをそれに合わせて作り直したいとき。

生成された `calibration.toml` の先頭には、ファイルの来歴を示すコメントヘッダが付きます。docs を見なくてもファイルを開いた人がここに辿り着けるようにしてあります。`floor_critical` / `floor_ok` のオーバーライドは `calibration.toml` ではなく `config.toml` 側に置いてください — そうしないと `heal calibrate --force` で消えてしまいます。

## キャッシュを覗く

スクリプト用の契約は `heal status --json` です。直接オンディスク状態を覗きたい場合は、`.heal/findings/` 配下にフラットな成果物が 3 つ置かれます。

| ファイル                        | 役割                                                                  |
| ------------------------------- | --------------------------------------------------------------------- |
| `.heal/findings/latest.json`    | 現在の TODO リスト — `heal status --refresh` がリフレッシュ。         |
| `.heal/findings/fixed.json`     | `/heal-code-patch` が記録した修正の有界マップ。                       |
| `.heal/findings/regressed.jsonl`| 修正済みが再検出された監査トレイル。                                  |

これらはすべて素のファイルなので `jq` で直接読めます。

```sh
jq '.severity_counts' .heal/findings/latest.json
jq 'keys | length' .heal/findings/fixed.json     # 記録済み修正数
tail .heal/findings/regressed.jsonl
```

## `heal hook commit`

`heal init` がインストールする git の post-commit フックから自動的に呼ばれます。全オブザーバーを実行し、`Critical` と `High` の Finding を 1 行のナッジとして stdout に出します（Hotspot フラグ付きが先頭）。クールダウンはありません — 同じ問題は修正されるまで毎コミット出続けます。それが狙いです。ディスクには何も書きません — 出力はナッジのみ。

デバッグ用に手動で実行することもあります。

```sh
heal hook commit
```

## ヒント

- **`heal status` が標準ワークフローです。** 意味のあるコミットの後に実行して、キャッシュをリフレッシュし TODO リストの残りを確認します。
- **`heal diff`**（引数なし）はセッション中の進捗確認に便利です。HEAD とライブスキャンを比較できます。
- **post-commit フックは保持する。** 削除するとコミット後の Severity ナッジが出なくなりますが、`heal status` は引き続きオンデマンドで動きます。
