---
title: CLI
description: heal のサブコマンドを日々の重要度順に並べた一覧と、運用で使うコマンドの例。
---

`heal` は単一のバイナリです。すべての操作は以下のサブコマンドのどれかを通じて行います。引数の詳細は `heal --help` または `heal <subcommand> --help` を参照してください。

## ユーザー向けコマンド

日々使う操作は実質この 4 つです。

| コマンド      | 用途                                                                                                           |
| ------------- | -------------------------------------------------------------------------------------------------------------- |
| `heal init`   | カレントリポジトリに `.heal/` をセットアップし、calibrate して post-commit フックを設置。                      |
| `heal skills` | 同梱の Claude スキルセットのインストール／更新／状態確認／削除。                                               |
| `heal status` | 現在の TODO リストを表示する（`--refresh` で再スキャン）。`.heal/findings/` を読みます。                       |
| `heal diff`   | ライブ worktree と過去のコミットを比較する（デフォルトは calibration の基準 SHA）。findings の `git diff` 版。 |

## 自動化向けコマンド

git の post-commit フックや同梱の Claude スキル経由で、ユーザーの代わりに走るコマンドです。`--help` には表示されません。

| コマンド           | 駆動元                     | 用途                                                                     |
| ------------------ | -------------------------- | ------------------------------------------------------------------------ |
| `heal hook`        | git post-commit            | コミットごとにオブザーバーを実行し Severity ナッジを表示。               |
| `heal mark fix`    | `/heal-code-patch` スキル  | 1 コミット 1 finding の修正を記録して、次の `heal status` で整合させる。 |
| `heal mark accept` | `/heal-code-review` スキル | チームが「設計上のもので直さない」と判断した項目を記録する。              |
| `heal metrics`     | `/heal-code-review` スキル | 各メトリクスのサマリを毎回ワーキングツリーから再計算。                   |
| `heal calibrate`   | `/heal-setup` スキル      | Severity しきい値を現在のコードベース分布にリセット。                    |

`heal metrics` と `heal calibrate` をここに置いているのは、_いつ_ 走らせるかを同梱スキルが判断するからです。`/heal-code-review` は監査の途中で各メトリクスのサマリを参照し、`/heal-setup` は calibration ドリフトを監視して必要なら recalibrate を提案します。手動で叩くのは、Claude を介さずに生の出力が欲しいときだけです。

---

## `heal init`

git リポジトリ内で heal をブートストラップします。

```sh
heal init                # 対話モード — 検出した各エージェントごとに Y/N 確認
heal init --yes          # 検出した全エージェント分のスキルを抽出（プロンプトなし）
heal init --no-skills    # スキルは入れない（CI / AI エージェント未導入）
heal init --force        # 既存の config.toml / フックを上書き、スキルもリフレッシュ
heal init --explicit     # 全デフォルト値を config.toml に書き出す
```

デフォルトの `heal init` は **最小形式** で `config.toml` を書き出します — チームが実際にカスタマイズした値だけがファイルに残り、新規プロジェクトでは事実上の空ファイルになります。`--explicit` を付けるとデフォルトツリー全体を書き出すので、利用可能なすべてのチューニングノブを参照できる形になります。

`heal init` の処理:

1. `.heal/` を作成し、`config.toml`、`calibration.toml`、`findings/` を配置します。`config.toml`、`calibration.toml`、`findings/` の中身はすべて git に追跡されるので、同じコミット上のチームメイトは同じ Severity ラダーと解消キューを共有できます。
2. 全オブザーバを一度走らせ、メトリクスごとにコードベースのパーセンタイル分布を計算 — これが `calibration.toml` になります。
3. `.git/hooks/post-commit` をインストール(冪等 — 再インストールでも行が重複しません)。
4. `PATH` にある AI エージェント CLI ごとに、同梱スキルセットを当該エージェントのプロジェクトスコープ配置先に展開します:
   - `claude` → `.claude/skills/`
   - `codex`  → `.agents/skills/`

   TTY 環境ではエージェントごとに 1 回ずつ Y/N プロンプト(デフォルト `Y`)。`--yes` で全許諾、`--no-skills` で全スキップ。CLI が `PATH` にないエージェントは黙ってスキップ(展開してもそのエージェントから呼べない)。

完了時には "Installed:" サマリで、生成したすべてのファイル(config、calibration、post-commit フック、エージェントごとのスキルパスまたはスキップ理由)を一覧表示します。

再実行は安全です。`--force` を付けない限り `config.toml` はそのまま残ります。post-commit フックは heal の目印を持つときだけ置き換えられます。heal 由来でない `post-commit` フックがすでに存在する場合は触りません(上書きするには `--force`)。

## `heal skills`

検出した全エージェントの同梱スキルセットを管理します。各サブコマンドは `--target <detected|claude|codex|all>` を受け付けます(デフォルトは `detected` で `heal init` と同じ挙動):

```sh
heal skills install                  # PATH にある全エージェント向けに展開
heal skills install --target codex   # `.agents/skills/` だけ
heal skills install --target all     # 検出有無に関わらず全 target に展開
heal skills update                   # heal バイナリ更新後にリフレッシュ
heal skills status                   # ターゲット別の installed バージョンと drift
heal skills uninstall --target all   # 全 tree を削除
```

スキルセットは `heal` バイナリに同梱されているので、各サブコマンドは常にバイナリに対応するバージョンに対して動きます。`update` はドリフト認識付きで、手編集されたファイルはターゲット単位で残します(`--force` で上書き可)。Claude target の `install` / `update` は `.claude/settings.json` から legacy な `heal hook edit` / `heal hook stop` エントリも掃除します(Codex target には対応する settings ファイルがないため何もしません)。

同梱されるスキルは 11 個、機能ファミリ別:

**Code(常時オン):**

- `/heal-code-review`(read-only) — `heal status --all --json` を取り込み、フラグ付きコードを深く読み、アーキテクチャ的な所見と優先順位付きリファクタ TODO リストを返します。
- `/heal-code-patch`(write) — TODO リストを Severity 順(`Critical 🔥` 先頭)に 1 コミット 1 finding ずつ解消。
- `/heal-cli` — `heal` CLI の簡潔なリファレンス。
- `/heal-setup` — セットアップウィザード。calibrate → strictness 選択 → `config.toml` 書き出し のあと、オプションの `[features.docs]` / `[features.test]` を有効化するかを順に確認し、有効化を選んだ場合は `/heal-doc-pair-setup` / `/heal-test-reporter-setup` まで連携します。calibration ドリフトを検知して `heal calibrate --force` も提案します。

**`[features.docs]`**(オプトイン):

- `/heal-doc-pair-setup`(`.heal/doc_pairs.json` を書く) — doc ⇔ src のペアを検出します。
- `/heal-doc-scaffold`(`[features.docs] scaffold_root` 配下に書く) — コードベースのシグナルだけを根拠に、ドキュメントツリーを新規生成します。
- `/heal-doc-review`(read-only) — Diátaxis レンズで docs スライスを監査します。
- `/heal-doc-patch`(write) — 内部リンク切れ、参照切れの識別子、孤立ページ、解決可能な TODO を 解消します。

**`[features.test]`**(オプトイン):

- `/heal-test-reporter-setup`(read-only) — lcov リポータと CI の設定を提案します。
- `/heal-test-review`(read-only) — テストピラミッドのレンズで test スライスを監査します。
- `/heal-test-patch`(write) — カバレッジ未達の hot path、ドリフトしたテスト、理由が成立しなくなった skip テストを 解消します。

スキルの仕様詳細は [Code › スキル](/heal/ja/code/skills/)、[Test › スキル](/heal/ja/test/skills/)、[Docs › スキル](/heal/ja/docs/skills/) を参照。

## `heal status`

各オブザーバを実行し、Finding を Severity 分類して、`/heal-code-review` が監査し `/heal-code-patch` が解消する TODO リストを書き出します。

```sh
heal status                              # キャッシュを再描画（デフォルト）
heal status --refresh                    # 再スキャンしてキャッシュを上書き
heal status --metric lcom                # LCOM の Finding のみ
heal status --metric coverage-pct        # カバレッジ findings のみ（[features.test]）
heal status --metric doc-drift           # doc-drift findings のみ（[features.docs]）
heal status --severity critical          # Critical のみ（`--all` で以上を含む）
heal status --feature code               # code ファミリのみ表示(test / docs を抑制)
heal status --feature test               # test ファミリのみ([features.test])
heal status --feature docs               # docs ファミリのみ([features.docs])
heal status --path src/payments          # パスプレフィックスで絞る(v0.4 以前は --feature)
heal status --all                        # Medium / Ok と低 Severity の Hotspot セクションを表示
heal status --top 5                      # 各 Severity バケットを 5 行で打ち切り
heal status --no-pager                   # ページャを通さず stdout に直接書く
heal status --json                       # 機械可読な形式を stdout へ
```

stdout がターミナルのときは `$PAGER`(または `less`)にパイプします(`git diff` / `git log` と同じ慣習)。`--no-pager` を渡すか、出力をパイプ(リダイレクト、`| cat`、CI ログ)するとページャは自動的にスキップされます。`--json` は常に raw のまま stdout に出します。

デフォルトの `heal status` はキャッシュ済み TODO の読み取り専用描画です。キャッシュが温まっていれば実質コスト 0 で動きます。`--refresh` を指定すると初めてキャッシュを破棄して再スキャン・上書きします(このパスだけが書き込みを行います)。`heal init` 直後などキャッシュがないときは最初の実行で自動的にスキャンするので、フラグなしでも問題なく動きます。

出力は Finding を `🔴 Critical 🔥 / 🔴 Critical / 🟠 High 🔥 / 🟠 High / 🟡 Medium / ✅ Ok` の下にグループ化し(最後の 2 つは `--all` が必要)、ファイル単位に 1 行へ集約します。最後に `Goal: 0 Critical, 0 High` と `claude /heal-code-patch` を指す "next steps" の行が続きます。`--all` を指定すると、「Severity は低いが上位 10% の Hotspot」に該当するファイルを別セクション(`Ok / Medium 🔥`)で追加表示します。

## `heal diff`

ライブ worktree と指定した過去コミットでの finding を比較します。デフォルト ref は calibration の基準 SHA(`heal init` / `heal calibrate --force` が記録した `meta.calibrated_at_sha`)で、記録されていないときは `HEAD` にフォールバックします。「Progress: N% complete」が「calibration からどれだけ 解消したか」として自然に読めるようにしているためです。

```sh
heal diff                              # ライブ vs calibration 基準
heal diff HEAD                         # ライブ vs 直近のコミット
heal diff main                         # ライブ vs main
heal diff v0.2.1                       # ライブ vs v0.2.1 タグ
heal diff HEAD~5                       # ライブ vs 5 コミット前
heal diff --all                        # Improved + Unchanged と High 未満のエントリも表示
heal diff --no-pager                   # ページャを通さず stdout に直接書く
heal diff --json                       # 機械可読な形式
```

`<git-ref>` には `git rev-parse` で解釈できるものを渡せます。heal は対象 ref を **現在の** `config.toml` / `calibration.toml` で再評価するので、apples-to-apples の比較になります(当時の評価ではなく、いまのルールで過去と現在を見る形です)。

ターミナル出力時のページャ動作は `heal status` と同じです。`--no-pager` で直接 stdout に出せます。

出力バケットは Resolved / Regressed / Improved / New / Unchanged + 進捗パーセンテージです。右辺は **常にワーキングツリーの即席スキャン** で、永続化されません。

人間向けレンダラはデフォルトで `from`/`to` のいずれもが High 未満のエントリを隠し、`[N entries below High hidden — pass --all]` というフッターを出します(ノイズの多い baseline で実行可能な行が埋もれないようにするためです)。`--all` を渡すとこの絞り込みが外れ、Improved / Unchanged バケットも一緒に表示されます。`--json` 出力は常にフィルタなしで、skill や CI からは全行が見えます。

巨大なリポジトリではこの比較が高コストになります。`config.toml` の `[diff]` で LOC 上限を設定でき、超過時は手動 2 ブランチ手順に切り替わります。詳しくは [Code › 設定](/heal/ja/code/configuration/#diff) を参照。

## `heal metrics`

```sh
heal metrics
heal metrics --json
heal metrics --metric complexity
heal metrics --metric lcom
heal metrics --metric coverage-pct
heal metrics --metric doc-freshness
heal metrics --no-pager
```

有効化された各メトリクスのサマリ(主言語、worst-N の複雑な関数、トップ Hotspot、最も分割可能なクラスなど)を表示します。`--metric <name>` で出力を単一のオブザーバに絞り込めます。指定できる名前:

- **Code**(常時利用可): `loc`、`complexity`、`churn`、`change-coupling`、`duplication`、`hotspot`、`lcom`。
- **`[features.docs]`**(有効化時): `doc-freshness`、`doc-drift`、`doc-coverage`、`doc-link-health`、`orphan-pages`、`todo-density`、`doc-hotspot`。
- **`[features.test]`**(有効化時): `coverage-pct`、`skip-ratio`、`test-hotspot`。

`--json` は同じデータを機械可読な JSON で出力するので、`jq` でのパイプ処理に向きます。

ターミナル出力時のページャ動作は `heal status` / `heal diff` と同じです。`--no-pager` で stdout に直接書き出せます。

呼び出しごとにワーキングツリーから再計算します。履歴は保持しないので、過去との差分は出ません。

## `heal calibrate`

```sh
heal calibrate            # calibration.toml が無ければ作成、あれば no-op
heal calibrate --force    # 常に再スキャンして calibration.toml を上書き
```

heal は **絶対に** 自動で recalibrate しません。コードベースを実際に改善するリファクタが、暗黙のうちにゴールポストを動かしてしまうのを避けるためです。`--force` を実行するのは次の場面です:

- 大きな構造変更で分布が変わったとき(`/heal-setup` スキルが監視して提案します)。
- `config.toml` の `floor_critical` / `floor_ok` を変えて、パーセンタイルラダーを合わせて作り直したいとき。

生成された `calibration.toml` の先頭には、ファイルの来歴を示すコメントヘッダが付きます。ファイルを開いただけでドキュメントなしに来歴をたどれるようにするためです。`floor_critical` / `floor_ok` の上書きは `calibration.toml` ではなく `config.toml` 側に置いてください。さもないと `heal calibrate --force` で消えてしまいます。

## キャッシュを覗く

スクリプト用の契約は `heal status --json` です。直接オンディスク状態を覗きたい場合は、`.heal/findings/` 配下にフラットな成果物が 3 つ置かれています:

| ファイル                         | 役割                                                          |
| -------------------------------- | ------------------------------------------------------------- |
| `.heal/findings/latest.json`     | 現在の TODO リスト — `heal status --refresh` がリフレッシュ。 |
| `.heal/findings/fixed.json`      | `/heal-code-patch` が記録した修正の有界マップ。               |
| `.heal/findings/regressed.jsonl` | 修正済みが再検出された監査トレイル。                          |

これらはすべて素のファイルなので `jq` で直接読めます。

```sh
jq '.severity_counts' .heal/findings/latest.json
jq 'keys | length' .heal/findings/fixed.json     # 記録済み修正数
tail .heal/findings/regressed.jsonl
```

## `heal hook commit`

`heal init` がインストールする git の post-commit フックから自動的に呼ばれます。全オブザーバを実行し、`Critical` と `High` の Finding を 1 行のナッジとして stdout に出します(Hotspot フラグ付きが先頭)。クールダウンはありません。同じ問題は修正されるまで毎コミット出続けます — それが狙いです。ディスクには何も書きません(出力はナッジのみ)。

`[features.test.coverage]` が有効で、High / Critical な `coverage_pct` finding が hotspot ファイル上にあるとき、ナッジには「N uncovered hotspot」をカウントするインデント付き 2 行目が追加されます。「次のテストはここに書くべき」の最短リマインダです。

デバッグ用に手動で実行することもあります。

```sh
heal hook commit
```

## ヒント

- **`heal status` が標準ワークフローです。** 意味のあるコミットの後に実行して、キャッシュをリフレッシュし TODO リストの残りを確認します。
- **`heal diff`**（引数なし）は calibration 基準との進捗確認に便利です。「% complete」が「calibration からどれだけ 解消したか」を表します。直近コミットとの比較がほしいときは `heal diff HEAD` を渡します。
- **post-commit フックは保持する。** 削除するとコミット後の Severity ナッジが出なくなりますが、`heal status` は引き続きオンデマンドで動きます。
