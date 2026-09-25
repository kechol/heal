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
| `heal doctor` | セットアップのどこまでが済んでいて、何が残っているかを確認する。                                               |
| `heal status` | 現在の TODO リストを表示する（`--refresh` で再スキャン）。`.heal/findings/` を読みます。                       |
| `heal diff`   | ライブ worktree と過去のコミットを比較する（デフォルトは calibration の基準 SHA）。findings の `git diff` 版。 |

Claude のスキルは CLI には含まれず、Claude Code プラグインとして配布しています([インストール](/heal/ja/installation/#claude-code-プラグイン) を参照)。`heal skills uninstall` は、heal 0.6 以前がプロジェクトにコピーしたスキルのフォルダを消します。

opt-in の [Semantic (Jev)](/heal/ja/semantic/) を有効にすると、コマンドが 2 つ増えます。heal のコマンドのうち、ネットワークにつながるのはこの 2 つだけです。

| コマンド            | 用途                                                                                             |
| ------------------- | ------------------------------------------------------------------------------------------------ |
| `heal semantic ask` | heal が選んだコード・テスト・ドキュメントについて Jev に問い合わせ、答えを `.heal/` に保存する。 |
| `heal auth jev`     | Jev の API キーを保存・確認・削除する（`set` / `status` / `clear`）。                            |

## 自動化向けコマンド

git の post-commit フックや Claude のスキル経由で、ユーザーの代わりに走るコマンドです。`heal hook` と `heal mark` は `--help` には表示されません。

| コマンド           | 駆動元                                        | 用途                                                                   |
| ------------------ | --------------------------------------------- | ---------------------------------------------------------------------- |
| `heal hook`        | git post-commit                               | コミットごとにオブザーバーを実行し Severity ナッジを表示。             |
| `heal mark fix`    | `/heal:refactor`、`/heal:docs`、`/heal:tests` | コミットで直した Finding を記録して、次の `heal status` で整合させる。 |
| `heal mark accept` | `/heal:refactor`、`/heal:docs`、`/heal:tests` | チームが「設計上のもので直さない」と判断した項目を記録する。           |
| `heal metrics`     | `/heal:setup`                                 | 各メトリクスのサマリを毎回ワーキングツリーから再計算。                 |
| `heal calibrate`   | `/heal:setup`                                 | Severity しきい値を現在のコードベース分布にリセット。                  |

`heal metrics` と `heal calibrate` をここに置いているのは、_いつ_ 走らせるかをスキルが判断するからです。`/heal:setup` は設定を調整するときに各メトリクスのサマリを参照し、コードベースが十分に動いたと `heal doctor` が報告したら recalibrate を提案します。手動で叩くのは、Claude を介さずに生の出力が欲しいときだけです。

---

## `heal init`

git リポジトリ内で heal をブートストラップします。

```sh
heal init                # .heal/ を作り、calibrate し、フックを入れる
heal init --force        # 既存の config.toml・calibration・フックを上書き
heal init --explicit     # 全デフォルト値を config.toml に書き出す
```

デフォルトの `heal init` は **最小形式** で `config.toml` を書き出します — チームが実際にカスタマイズした値だけがファイルに残り、新規プロジェクトでは事実上の空ファイルになります。`--explicit` を付けるとデフォルトツリー全体を書き出すので、利用可能なすべてのチューニングノブを参照できる形になります。

`heal init` の処理:

1. `.heal/` を作成し、`config.toml`、`calibration.toml`、`findings/` を配置します。`config.toml`、`calibration.toml`、`findings/` の中身はすべて git に追跡されるので、同じコミット上のチームメイトは同じ Severity ラダーと解消キューを共有できます。
2. 全オブザーバを一度走らせ、メトリクスごとにコードベースのパーセンタイル分布を計算 — これが `calibration.toml` になります。
3. `.git/hooks/post-commit` をインストール(冪等 — 再インストールでも行が重複しません)。

完了時には "Installed:" サマリで、書いたもの・残したもの(config、calibration、post-commit フック)と、Claude Code プラグインの入れ方を表示します。スキルを入れることはしません。`--yes` と `--no-skills` は古いスクリプトが動き続けるように受け付けますが、何もしません。

再実行は安全です。`--force` を付けない限り、既存の `config.toml` と `calibration.toml` はそのまま残ります。post-commit フックは heal の目印を持つときだけ置き換えられます。heal 由来でない `post-commit` フックがすでに存在する場合は触りません(上書きするには `--force`)。heal 0.6 以前のスキルのフォルダが残っていれば、サマリでそう伝えます。

## `heal doctor`

```sh
heal doctor          # 項目ごとに 1 行。足りないものには次にやることを添える
heal doctor --json   # 同じ内容を JSON で(/heal:setup はこれを読む)
```

heal が必要とするものを確認し、項目ごとに `ok`・`todo`・`warn`・`off`(有効にしていない任意の機能)・`error` のどれかと、それを直すコマンドやスキルを報告します。

- `.heal/config.toml` があり、読み込めるか。
- `.heal/calibration.toml` があり、今のコードベースに合っているか。calibration 後に 200 を超えるコミットが入った、ファイル数が 20% を超えて変わった、修正を 10 件以上記録して Critical / High が残っていない、のどれかに当てはまると知らせます。heal が自分で calibrate し直すことはありません。納得したら `heal calibrate --force` を実行してください。
- post-commit フックが入っているか。
- 有効にした任意の機能に必要なものがそろっているか。docs なら doc pairs、coverage なら lcov ファイル、semantic なら API キーと concept の一覧です。
- 古い heal がプロジェクトにコピーしたスキルのフォルダが残っていないか。

`heal doctor` は読むだけで、何も変更せず、ネットワークにもつなぎません。API に対してキーを確かめるときは `heal auth jev status` を使います。`/heal:setup` はこの報告から始めるので、再実行しても足りないことしかしません。

## `heal skills uninstall`

スキルは Claude Code プラグインとして配布するようになったので、CLI がスキルを入れたり更新したりすることはもうありません。残っているサブコマンドは、スキルを各プロジェクトにコピーしていた heal 0.6 以前の後片付けだけです。

```sh
heal skills uninstall          # 古い heal のスキルのフォルダを消す
heal skills uninstall --json   # 消したものを JSON で出す
```

`.claude/skills/heal-*` と `.agents/skills/heal-*` のうち、heal 自身が書いた決まった名前のフォルダだけを消し(自分で作ったスキルは残ります)、`.claude/settings.json` から古い heal のエントリを取り除きます。終わったら削除をコミットしてください。

`heal skills install`・`update`・`status` は、古いスクリプトが分かりやすく失敗するように受け付けたうえで、プラグインの入れ方を表示して終了コード 1 で終わります。

## `heal status`

各オブザーバを実行し、Finding を Severity 分類して、`/heal:refactor`・`/heal:docs`・`/heal:tests` の各スキルが使う TODO リストを書き出します。

```sh
heal status                              # キャッシュを再描画（デフォルト）
heal status --refresh                    # 再スキャンしてキャッシュを上書き
heal status --metric lcom                # LCOM の Finding のみ
heal status --metric coverage-pct        # カバレッジ findings のみ（[features.test]）
heal status --metric doc-drift           # doc-drift findings のみ（[features.docs]）
heal status --severity high              # High と Critical（--all でもこの下限は下がらない）
heal status --feature code               # code ファミリのみ表示(test / docs を抑制)
heal status --feature test               # test ファミリのみ([features.test])
heal status --feature docs               # docs ファミリのみ([features.docs])
heal status --path src/payments          # パスプレフィックスで絞る(v0.4 以前は --feature)
heal status --all                        # Advisory、Medium、Ok、accepted セクションも表示
heal status --top 5                      # 各 Tier/Severity バケットを 5 行で打ち切り
heal status --no-pager                   # ページャを通さず stdout に直接書く
heal status --json                       # 機械可読な形式を stdout へ
```

stdout がターミナルのときは `$PAGER`(または `less`)にパイプします(`git diff` / `git log` と同じ慣習)。`--no-pager` を渡すか、出力をパイプ(リダイレクト、`| cat`、CI ログ)するとページャは自動的にスキップされます。`--json` は常に raw のまま stdout に出します。

デフォルトの `heal status` は鮮度が有効なキャッシュを再利用するため、温まっていれば実質コスト 0 です。キャッシュが欠けているか古ければ自動的に再スキャンして置き換え、`--refresh` は鮮度にかかわらず同じ再スキャンと書き込みを強制します。

鮮度は HEAD と clean-worktree gate だけでなく、有効な非 git 観測入力も含めて判定します。ignored な LCOV や doc-pair を更新すると、HEAD が同じでも cache は無効です。mtime と checkout の絶対パスは使いません。human/JSON の coverage provenance は `missing` / `read_error` / `partial` / `complete` を区別します。LCOV にない production ファイルは未計測であり、0% とは判定せず reporter/package scope の確認へ案内します。

出力は Finding を有効 Drain Tier と Severity でグループ化し(低優先度セクションは `--all` が必要)、ファイル単位に 1 行へ集約します。Hotspot は全行 hot のセクションまたは混在行の `🔥` で表示します。優先順は Tier、Severity、同一ファミリの `hotspot_score` 降順、metric/path/id の tie-break です。Code、Test、Docs の生スコアは相互比較せず、確率や修正効果の保証でもありません。

`--severity` は常に最小 Severity の下限です。`--all` はその下限以上にある通常非表示のセクションを表示できますが、下限未満の finding は復元しません。

## `heal diff`

ライブ worktree と指定した過去コミットでの finding を比較します。デフォルト ref は calibration の基準 SHA(`heal init` / `heal calibrate --force` が記録した `meta.calibrated_at_sha`)で、記録されていないときは `HEAD` にフォールバックします。「Progress: N% complete」が「calibration からどれだけ 解消したか」として自然に読めるようにしているためです。

```sh
heal diff                              # ライブ vs calibration 基準
heal diff HEAD                         # ライブ vs 直近のコミット
heal diff main                         # ライブ vs main
heal diff v0.2.1                       # ライブ vs v0.2.1 タグ
heal diff HEAD~5                       # ライブ vs 5 コミット前
heal diff --all                        # Improved + Unchanged と High 未満のエントリも表示
heal diff --hide-accepted              # `heal mark accept` 済みの行を隠す
heal diff --no-pager                   # ページャを通さず stdout に直接書く
heal diff --json                       # 機械可読な形式
```

`<git-ref>` には `git rev-parse` で解釈できるものを渡せます。heal は対象 ref を **現在の** `config.toml` / `calibration.toml` で再評価するので、apples-to-apples の比較になります(当時の評価ではなく、いまのルールで過去と現在を見る形です)。

ターミナル出力時のページャ動作は `heal status` と同じです。`--no-pager` で直接 stdout に出せます。

出力バケットは Resolved / Regressed / Improved / New / Unchanged + 進捗パーセンテージです。右辺は **常にワーキングツリーの即席スキャン** で、永続化されません。

人間向けレンダラはデフォルトで `from`/`to` のいずれもが High 未満のエントリを隠し、`[N entries below High hidden — pass --all]` というフッターを出します(ノイズの多い baseline で実行可能な行が埋もれないようにするためです)。`--all` を渡すとこの絞り込みが外れ、Improved / Unchanged バケットも一緒に表示されます。`--json` 出力は常にフィルタなしで、skill や CI からは全行が見えます。

`heal mark accept` で受容済みの finding には `📌 accepted` マーカーが付き、New / Regressed の行が「把握済みで対応不要」だと一目で分かります。`--hide-accepted` を渡すとこれらの行ごと隠れ、対応が必要な行だけが残ります(`[N accepted entries hidden]` フッターで件数は見えます)。この絞り込みは `--all` とは独立に効きます。

coverage が有効なら、JSON は `from_coverage_observation` と `to_coverage_observation` を返します。両側の `missing` / `read_error` / `partial` / `complete` と入力一覧により、未計測と実測 0% / 100% を区別できます。

accepted finding の Severity が上昇するか、そのファミリの Hotspot が false から true になると、status、diff の現在側、post-commit hook に再レビュー通知が出ます。JSON は 1 つの `accepted_rereview` に 1 つまたは両方の理由を返します。accept は解除せず、finding を解消キューに戻しません。

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

- 大きな構造変更で分布が変わったとき(`heal doctor` が知らせ、`/heal:setup` が提案します)。
- `config.toml` の `floor_critical` / `floor_ok` を変えて、パーセンタイルラダーを合わせて作り直したいとき。

生成された `calibration.toml` の先頭には、ファイルの来歴を示すコメントヘッダが付きます。ファイルを開いただけでドキュメントなしに来歴をたどれるようにするためです。`floor_critical` / `floor_ok` の上書きは `calibration.toml` ではなく `config.toml` 側に置いてください。さもないと `heal calibrate --force` で消えてしまいます。

`[features.semantic]` が有効なら、`heal status --focus <file>` で、ファイルに書いた作業に合わせて並べられます（[Semantic (Jev)](/heal/ja/semantic/) を参照）。この場合は必ず再スキャンし、保存済みの TODO リストは更新しません。

## `heal semantic ask`

`[features.semantic] enabled = true` のときだけ使えます。何が送られるかは [Semantic (Jev)](/heal/ja/semantic/) を参照してください。

```sh
heal semantic ask --dry-run          # 計画と見積もりだけ。何も送らない
heal semantic ask                    # 有効なタスクをすべて問い合わせる
heal semantic ask --task <id>        # 1 つのタスクだけ（複数指定可）
heal semantic ask --refresh          # 答えが保存済みでも問い合わせ直す
heal semantic ask --prune            # どこからも参照されない答えを消す
heal semantic ask --check            # キーが使えるかの確認だけ
heal semantic ask --task focus --focus plan.md    # plan.md の作業に合わせて並べる
heal semantic ask --task verify_patch --diff HEAD~1..HEAD   # commit の範囲を判定する
heal semantic ask --json             # 実行結果を JSON で出す
```

終了コード `2` は、利用者にしか直せない問題を表します。機能が無効、API キーが未設定、キーが拒否された、設定した `model` を API が知らない、のいずれかです。

## `heal auth jev`

```sh
printf '%s\n' "$KEY" | heal auth jev set   # ユーザーの設定ファイルに保存（mode 600）
heal auth jev status                        # キーの出どころと、実際に使えるかの確認
heal auth jev status --offline              # 通信での確認を省く
heal auth jev clear                         # 保存したキーを消す
```

`TYPESAFE_API_KEY`（または `TYPESAFEAI_API_KEY`）が、保存したキーより優先されます。キーが `.heal/` の下に書かれることはありません。

## キャッシュを覗く

スクリプト用の契約は `heal status --json` です。直接オンディスク状態を覗きたい場合は、`.heal/findings/` 配下にフラットな成果物が 4 つ置かれています:

| ファイル                         | 役割                                                                      |
| -------------------------------- | ------------------------------------------------------------------------- |
| `.heal/findings/latest.json`     | 現在の TODO — fresh なら再利用し、stale/欠落時または `--refresh` で置換。 |
| `.heal/findings/fixed.json`      | スキルが `heal mark fix` で記録した修正の有界マップ。                     |
| `.heal/findings/accepted.json`   | `heal mark accept` で「直さない」と判断した finding の記録。              |
| `.heal/findings/regressed.jsonl` | 修正済みが再検出された監査トレイル。                                      |

2 つの JSON 表示は意図的に byte-for-byte では一致しません。`latest.json` はオブザーバの生レコードです。`heal status --json` は同じレコードschemaを使い、現在の accepted 状態と一時的な `accepted_rereview` 通知をoverlayしたうえで、指定された workspace、feature、metric、path、Severity のfilterを finding、再レビュー通知、その集計値へ適用します。coverage provenance 自体には Severity がなく、workspace/path の範囲に従い、Test 以外のfamilyまたはcoverage以外のmetricが指定された場合は省略します。

これらはすべて素のファイルなので `jq` で直接読めます。

```sh
jq '.severity_counts' .heal/findings/latest.json
jq 'keys | length' .heal/findings/fixed.json     # 記録済み修正数
tail .heal/findings/regressed.jsonl
```

## `heal hook commit`

`heal init` がインストールする git の post-commit フックから自動的に呼ばれます。全オブザーバを実行し、`Critical` と `High` の Finding を 1 行のナッジとして stdout に出します(Hotspot フラグ付きが先頭)。クールダウンはありません。同じ問題は修正されるまで毎コミット出続けます — それが狙いです。ディスクには何も書きません(出力はナッジのみ)。

`[features.test.coverage]` が有効で、High / Critical な `coverage_pct` finding が hotspot ファイル上にあるとき、ナッジには「N uncovered hotspot」をカウントするインデント付き 2 行目が追加されます。「次のテストはここに書くべき」の最短リマインダです。

coverage が欠落・読取不能・partial なとき、hook は未計測ファイルを uncovered Hotspot と解釈せず、status と同じ reporter/package scope の案内を出します。accepted の判断前提が変わった場合も再レビューを通知します。

デバッグ用に手動で実行することもあります。

```sh
heal hook commit
```

## ヒント

- **`heal status` が標準ワークフローです。** 意味のあるコミットの後に実行して、キャッシュをリフレッシュし TODO リストの残りを確認します。
- **`heal diff`**（引数なし）は calibration 基準との進捗確認に便利です。「% complete」が「calibration からどれだけ 解消したか」を表します。直近コミットとの比較がほしいときは `heal diff HEAD` を渡します。
- **post-commit フックは保持する。** 削除するとコミット後の Severity ナッジが出なくなりますが、`heal status` は引き続きオンデマンドで動きます。
