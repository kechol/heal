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
| `heal status`    | `.heal/findings/latest.json` のキャッシュを描画する（`--refresh` で再スキャン上書き）。「いまの TODO」。 |
| `heal diff`      | ライブ worktree とキャッシュ済 `FindingsRecord` の差分（デフォルト ref: `HEAD`）。`git diff` 風。       |
| `heal metrics`   | 各メトリクスのサマリを毎回ワーキングツリーから再計算して表示。                                        |
| `heal calibrate` | コードベース相対の Severity しきい値を再 calibrate する。                                             |

## 自動化向けコマンド

git の post-commit フックや `/heal-code-patch` スキルから自動的に呼ば
れます。通常は手動で叩きません。`--help` には表示されません。

| コマンド          | 呼び出し元                | 用途                                                                                      |
| ----------------- | ------------------------- | ----------------------------------------------------------------------------------------- |
| `heal hook`       | git post-commit           | オブザーバー実行と Severity ナッジ表示。                                                  |
| `heal mark-fixed` | `/heal-code-patch` スキル | コミットで Finding を直したことを `.heal/findings/fixed.json` に記録。                    |

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
   `findings/`、そして `findings/` を除外する `.gitignore` を配置
   （`config.toml` と `calibration.toml` は git で追跡され、チーム
   で同じ Severity ラダーを共有できます）。
2. 全オブザーバーを一度走らせ、メトリクスごとにコードベースのパー
   センタイル分布を計算 — これが `calibration.toml` になります
   （先頭には `heal calibrate --force` を案内するコメントヘッダ）。
3. `.git/hooks/post-commit` をインストール（冪等 — スクリプトには
   コメントの目印が付いており、再インストールでも行が重複しません）。
4. `claude` コマンドが `PATH` にあれば、同梱のスキルセットを
   `.claude/skills/` に展開するか確認します。プロンプトのデフォル
   トは `Y`。プロンプトを飛ばして必ずインストールするには
   `--yes`、確認なしで飛ばすには `--no-skills`。`claude` が `PATH`
   に **無い** 場合は、確認なしで黙ってスキップします（スキルを入れ
   ても話す相手がいないため）。
5. `.claude/settings.json` に古い heal が残した
   `heal hook edit` / `heal hook stop` エントリがあれば掃除します
   （現行 heal は PostToolUse / Stop フックを登録しません）。

完了時には "Installed:" サマリで、生成したすべてのファイルを一覧表
示します — config、calibration、post-commit フック、そして Claude
スキルのパス（またはスキップ理由）。

再実行は安全です。`--force` を付けない限り `config.toml` はそのま
ま残ります。post-commit フックは heal の目印を持つときのみ置き換え
られます。heal 由来でない `post-commit` フックがすでに存在する場合、
`heal init` はそれを残します — 上書きするには `--force`。

## `heal skills`

`.claude/skills/` 配下の同梱スキルセットを管理します。

```sh
heal skills install     # スキル展開（リポジトリごとに一度）
heal skills update      # heal バイナリを更新した後にリフレッシュ
heal skills status      # インストール済みと同梱版を比較
heal skills uninstall   # スキルとマニフェストを削除
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
- `/heal-code-patch`（write）— `.heal/findings/latest.json` を
  Severity 順（`Critical 🔥` 先頭）に 1 コミット 1 Finding ずつ消化。
- `/heal-cli` — `heal` CLI 表面の簡潔なリファレンス。
- `/heal-config` — calibrate して strictness レベルを訊き、
  `config.toml` を書き出す。あわせて `calibration.toml` のメタ情報
  と現在の `latest.json` / `fixed.json` を見て calibration ドリフ
  トを冪等に検知します。

`heal skills install`（と `heal init`）は、古い heal が残した
`heal hook edit` / `heal hook stop` エントリを `.claude/settings.json`
から掃除します — 現行 heal は Claude のフックを一切登録しません。
`uninstall` はマーケットプレイス時代のレガシーレイアウト（旧
`.claude/plugins/heal/`、`.claude-plugin/marketplace.json`、および
`heal@heal-local` 系の settings エントリ）も同時に掃除するので、
古い heal からの移行は uninstall + 再インストールで完結します。

スキルの仕様詳細は[Claude スキル](/heal/ja/claude-skills/)を参照。

## `heal status`

各オブザーバーを実行し、`calibration.toml` で Finding を Severity
分類して、`.heal/findings/latest.json` に書き出します（`/heal-code-review`
が監査し、`/heal-code-patch` が消化する TODO リスト）。

```sh
heal status                              # キャッシュを再描画（デフォルト）
heal status --refresh                    # 再スキャンして latest.json を上書き
heal status --metric lcom                # LCOM の Finding のみ
heal status --severity critical          # Critical のみ（`--all` で以上を含む）
heal status --feature src/payments       # 特定パスプレフィックスに絞る
heal status --all                        # Medium / Ok と低 Severity の Hotspot セクションを表示
heal status --top 5                      # 各 Severity バケットを 5 行で打ち切り
heal status --json                       # FindingsRecord 形式を stdout へ
```

デフォルトの `heal status` は `.heal/findings/latest.json` を読み出
すだけのリードオンリー描画なので、キャッシュが温まっていれば実質コ
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
複雑な関数、トップ Hotspot、最も分割可能なクラスなど — を表示しま
す。`--metric <name>` で出力を単一のオブザーバーに絞り込めます。
指定できる名前: `loc`、`complexity`、`churn`、`change-coupling`、
`duplication`、`hotspot`、`lcom`。`--json` は同じデータを機械可読
な JSON で出力するため、`jq` でのパイプ処理に向きます。

`heal metrics` は呼び出しごとにすべてをワーキングツリーから再計算
します — 履歴ストリームを保持しないため、前スナップショットからの
差分は表示しません。

## `heal calibrate`

```sh
heal calibrate            # calibration.toml が無ければ作成、あれば no-op
heal calibrate --force    # 常に再スキャンして calibration.toml を上書き
```

heal は **絶対に** 自動で recalibrate しません。

- `.heal/calibration.toml` が **無い** とき、`heal calibrate` は再
  スキャンしてファイルを書き出します。（通常は `heal init` がブー
  トストラップ時に作成するので、このパスは init 前のみ。）
- ファイルが **ある** ときのデフォルト実行は、ファイル存在を報告
  し、再生成のヒントとして `--force` を案内するだけです。

ドリフト検知はもう CLI 側に存在しません。`/heal-config` スキルが
`calibration.toml.meta.calibrated_at_sha` / `calibrated_at_files`
を現在の `.heal/findings/latest.json` と `.heal/findings/fixed.json`
と突き合わせ、必要に応じて `heal calibrate --force` を推奨します。

生成された `calibration.toml` の先頭には、ファイルの来歴と再生成
コマンドを示すコメントヘッダが付きます。docs を見なくてもファイル
を開いた人が辿れるようにしてあります。`floor_critical` / `floor_ok`
のオーバーライドは `config.toml` 側に置いてください（`heal calibrate
--force` で消えないようにするため）。

## キャッシュを覗く

`.heal/findings/` がオンディスクの唯一の状態で、フラットな成果物
が 3 つ置かれます。

| ファイル                        | 形式                                  | 役割                                                                         |
| ------------------------------- | ------------------------------------- | ---------------------------------------------------------------------------- |
| `.heal/findings/latest.json`    | `FindingsRecord`（単一オブジェクト）     | 現在の TODO リスト — `heal status --refresh` がリフレッシュ。                |
| `.heal/findings/fixed.json`     | `BTreeMap<finding_id, FixedFinding>`  | `heal mark-fixed` が記録した修正の有界マップ。                               |
| `.heal/findings/regressed.jsonl`| 追記専用 JSON-lines                   | 修正済みが再検出された監査トレイル。                                         |

これらはすべて素のファイルなので `jq` で直接読めます。

```sh
jq '.severity_counts' .heal/findings/latest.json
jq 'keys | length' .heal/findings/fixed.json     # 記録済み修正数
tail .heal/findings/regressed.jsonl
```

イベントログも履歴ストリームもなく、`heal logs` /
`heal snapshots` / `heal checks` / `heal compact` といった閲覧コマ
ンドもありません — イベントログ廃止と一緒に削除されました。

## `heal diff`

ライブ worktree とキャッシュ済 `FindingsRecord`（`head_sha` が解決した
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

`<git-ref>` には `git rev-parse` で解釈できるものを渡せます。
`.heal/findings/latest.json` の `head_sha` と一致する場合は、その
キャッシュを使って即座に diff を出します。一致しない場合は
`git worktree` ベースのフォールバックに切り替わり、対象 ref を一時
worktree に展開してそこでオブザーバーを走らせ、ライブと比較します。

worktree モードは `config.toml` の `[diff].max_loc_threshold`
（デフォルト `200_000` LOC）でゲートされます。閾値を超えると
`heal diff` は終了コード 2 で終了し、worktree を作る代わりに
2 ブランチを手動で並べる手順を表示します。

右辺は **常にワーキングツリーの即席スキャン** で、永続化されません。
出力 bucket は Resolved / Regressed / Improved / New / Unchanged ＋
進捗パーセンテージです。

`.heal/findings/latest.json` の writer は `heal status` のみ。
`heal mark-fixed` がもう一つの writer で、`fixed.json` に追記し、
再検出時には `regressed.jsonl` に移します。`heal diff` は完全リード
オンリーです。

---

## `heal hook`（自動化）

このコマンドは git の post-commit フックから自動的に呼ばれます。
デバッグのために手動実行が役立つことがあります。

```sh
heal hook commit          # post-commit: オブザーバー実行、ナッジ表示
heal hook edit            # 互換のためのレガシー no-op
heal hook stop            # 互換のためのレガシー no-op
```

`heal hook commit` は全オブザーバーを実行し、`Critical` と `High`
の Finding をすべて stdout に出します（`Medium` と `Ok` は静かなま
ま）。Hotspot フラグ付きのエントリが先頭に来ます。クールダウンは
ありません — 同じ問題は修正されるまで毎コミット出続けます。それが
狙いです。ディスクには何も書きません — 出力はナッジのみ。

`heal hook edit` と `heal hook stop` は、古い heal が
`.claude/settings.json` に登録した古いエントリがエラーで落ちないよ
うにするための静かな no-op です。新規インストールでは登録されません。

---

## 古い heal からの移行

既存リポジトリには、現行 heal がもう書き込まないし読まない古い状
態ディレクトリが残っているはずです。手動で削除して問題ありません。

```sh
rm -rf .heal/snapshots .heal/logs .heal/docs .heal/reports .heal/checks
```

`.heal/checks/` は `heal init` を再実行すると `.heal/findings/` に
名前変更されますが、旧ディレクトリはそのまま残るので削除してくだ
さい。アップグレード後、新しい `.heal/.gitignore` を取り込むため
に `heal init` を一度実行しておくと安全です。

---

## ヒント

- **`heal status` が標準ワークフローです。** 意味のあるコミットの後
  に実行して、キャッシュをリフレッシュし TODO リストの残りを確認し
  ます。
- **`heal diff`**（引数なし）はセッション中の進捗確認に便利です。
  HEAD のキャッシュとライブスキャンを比較できます。
- **post-commit フックは保持する。** 削除するとコミット後の Severity
  ナッジが出なくなりますが、`heal status` は引き続きオンデマンドで
  動きます。
