---
title: CLI
description: heal のサブコマンドを、日々よく使う順に、普段の操作の例とあわせて紹介します。
---

`heal` は 1 つのバイナリで、操作はすべて下のサブコマンドのどれかを通して行います。引数の一覧は `heal --help` または `heal <subcommand> --help` で確認できます。

## ユーザー向けコマンド

普段、実際に打つのはこれらのコマンドです。

| コマンド      | 役割                                                                                                     |
| ------------- | -------------------------------------------------------------------------------------------------------- |
| `heal init`   | 今いるリポジトリに `.heal/` を作り、calibration を行い、post-commit フックを設置する。                   |
| `heal doctor` | セットアップのどこまでが済んでいて、何が残っているかを確認する。                                         |
| `heal status` | 今の TODO リストを表示する（または作り直す）。`.heal/findings/` を読む。                                 |
| `heal diff`   | 今の作業ツリーを以前のコミット（既定は calibration の基準点）と比べる。Finding を対象にした `git diff`。 |

Claude のスキルは CLI には含まれず、Claude Code プラグインとして配布しています（[インストール](/heal/ja/installation/#claude-code-プラグイン) を参照）。`heal skills uninstall` は、heal 0.6 以前がプロジェクトにコピーしたスキルのフォルダを消します。

オプトインの [Semantic (Jev)](/heal/ja/semantic/) を有効にすると、コマンドが 2 つ加わります。heal の中でネットワークにつながるのは、この 2 つだけです。

| コマンド            | 役割                                                                                                 |
| ------------------- | ---------------------------------------------------------------------------------------------------- |
| `heal semantic ask` | heal が選んだコード・テスト・ドキュメントについて Jev に問い合わせ、答えを `.heal/` の下に保存する。 |
| `heal auth jev`     | Jev の API キーを保存・確認・削除する（`set` / `status` / `clear`）。                                |

## 自動化向けコマンド

これらは、git の post-commit フックや Claude のスキルから、あるいはコードベースが大きく変わって見直しが必要になったときに、自動で実行されます。`heal hook` と `heal mark` は `--help` には表示されません。

| コマンド           | 呼び出し元                                    | 役割                                                                                |
| ------------------ | --------------------------------------------- | ----------------------------------------------------------------------------------- |
| `heal hook`        | git の post-commit                            | コミットのたびにオブザーバを実行し、Severity の通知を出す。                         |
| `heal mark fix`    | `/heal:refactor`、`/heal:docs`、`/heal:tests` | Finding を直したコミットを記録し、次の `heal status` で突き合わせられるようにする。 |
| `heal mark accept` | `/heal:refactor`、`/heal:docs`、`/heal:tests` | チームが変えないと決めた、本質的な Finding を記録する。                             |
| `heal metrics`     | `/heal:setup`                                 | メトリクスごとの集計を、実行のたびに計算し直す。                                    |
| `heal calibrate`   | `/heal:setup`                                 | Severity のしきい値を、今のコードベースの分布に合わせて作り直す。                   |

`heal metrics` と `heal calibrate` をここに入れているのは、いつ実行するかをスキルが決めるからです。`/heal:setup` は設定を調整するときにメトリクスごとの集計を読み、コードベースが十分に変わったと `heal doctor` が報告すれば calibration のやり直しを勧めます。手で実行するのは、Claude を通さずに生の出力を見たいときだけで十分です。

---

## `heal init`

git リポジトリの中で heal を使える状態にします。

```sh
heal init                # .heal/ を作り、calibration を行い、フックを設置する
heal init --force        # 既存の config.toml、calibration、フックを上書きする
heal init --explicit     # すべての既定値を config.toml に書き出す（長い形式）
```

`heal init` は、既定では `config.toml` を**最小の形**で書きます。ディスクに残るのは、利用者が実際に変更した項目だけなので、新しいプロジェクトではほぼ空のファイルになります。`--explicit` なら既定値をすべて書き出すので、どんな設定項目があるかを見渡すリファレンスとしても使えます。

`heal init` が行うことは次の 3 つです。

1. `config.toml`、`calibration.toml`、`findings/` を含む `.heal/` を作ります。`config.toml`、`calibration.toml`、`findings/` の下のキャッシュはどれも git で管理するので、同じコミットにいるチームメンバーは同じ Severity の段階と解消キューを見ます。
2. すべてのオブザーバを 1 回実行し、メトリクスごとにコードベースのパーセンタイル分布を計算します。これが `calibration.toml` になります。
3. `.git/hooks/post-commit` を設置します。何度設置しても行は重複しません。

終わると、`heal init` は「Installed:」の一覧を出します。設定、calibration、post-commit フックのそれぞれについて書き込んだか残したかを示し、Claude Code プラグインの入れ方も添えます。スキルそのものは入れません。`--yes` と `--no-skills` は、古いスクリプトが動き続けるように今も受け付けますが、何もしません。

何度実行しても安全です。既存の `config.toml` と `calibration.toml` は `--force` を付けない限り残し、post-commit フックは heal の目印があるときだけ更新します。heal 以外の `post-commit` フックがすでにあれば、`heal init` はそれに触りません。上書きするには `--force` を付けます。heal 0.6 以前のスキルのフォルダがプロジェクトに残っていれば、一覧でそのことを知らせます。

## `heal doctor`

```sh
heal doctor          # 領域ごとに 1 行。足りないものには次の手順を添える
heal doctor --json   # 同じ内容を JSON で出す（/heal:setup が読む形式）
```

heal に必要なものを確認し、領域ごとに `ok`、`todo`、`warn`、`off`（有効にしていないオプション機能）、`error` のどれかで報告します。直すためのコマンドやスキルも添えます。確認するのは次の項目です。

- `.heal/config.toml` があり、読み込めるか。
- `.heal/calibration.toml` があり、今のコードベースにまだ合っているか。calibration の後に 200 を超えるコミットが入った、ファイル数が 20% を超えて増減した、修正を 10 件以上記録したのに Critical / High が 1 件も残っていない、のどれかに当てはまると知らせます。heal が自分で calibration をやり直すことはありません。納得したら `heal calibrate --force` を実行してください。
- post-commit フックが設置されているか。
- 有効にしたオプション機能に必要なものがそろっているか。docs ならドキュメントのペア、カバレッジなら lcov ファイル、semantic なら API キーと概念の一覧です。
- 古いバージョンの heal がプロジェクトにコピーしたスキルのフォルダが残っていないか。

`heal doctor` は読むだけです。何も変更せず、ネットワークにもつながりません（キーが API で通るかは `heal auth jev status` で確かめます）。`/heal:setup` はこの報告を起点にするので、何度実行しても足りない作業だけを行います。

## `heal skills uninstall`

スキルは Claude Code プラグインとして配布するようになったので、CLI はもうスキルを入れたり更新したりしません。残っている唯一のサブコマンドは、スキルを各プロジェクトにコピーしていた heal 0.6 以前の後片付けをします。

```sh
heal skills uninstall          # 古い heal のスキルのフォルダを消す
heal skills uninstall --json   # 消したものを JSON で一覧にする
```

消すのは `.claude/skills/heal-*` と `.agents/skills/heal-*` のうち、heal 自身が書いた決まった名前のフォルダだけで、自分で書いたスキルは残ります。あわせて `.claude/settings.json` から古い heal のエントリを取り除きます。終わったら、削除をコミットしてください。

`heal skills install`、`update`、`status` は、古いスクリプトが分かりやすいメッセージで止まるように、今もコマンドとしては受け付けます。プラグインの入れ方を表示し、終了コード 1 で終わります。

## `heal status`

すべてのオブザーバを実行し、Finding ごとに Severity を決めて、`/heal:refactor`、`/heal:docs`、`/heal:tests` の各スキルが使う TODO リストを書き出します。

```sh
heal status                              # キャッシュ済みの TODO を表示する（既定）
heal status --refresh                    # 走査し直してキャッシュを上書きする
heal status --metric lcom                # LCOM の Finding だけ
heal status --metric coverage-pct        # カバレッジの Finding だけ（[features.test]）
heal status --metric doc-drift           # doc-drift の Finding だけ（[features.docs]）
heal status --severity high              # High と Critical。--all を付けてもこの下限は下がらない
heal status --feature code               # code ファミリだけ（test / docs を除く）
heal status --feature test               # test ファミリだけ（[features.test]）
heal status --feature docs               # docs ファミリだけ（[features.docs]）
heal status --path src/payments          # 1 つのパスの配下に絞る（v0.4 より前は --feature）
heal status --all                        # Advisory、Medium、Ok、受け入れ済みのセクションも表示する
heal status --top 5                      # Tier / Severity ごとのグループを 5 行までにする
heal status --no-pager                   # ページャを通さず標準出力に書く
heal status --json                       # 機械可読な形で標準出力に書く
```

標準出力が端末のときは、`heal status` は `$PAGER`（なければ `less`）を通して表示します。`git diff` や `git log` と同じ流儀です。ページャを使いたくなければ `--no-pager` を付けます。リダイレクトや `| cat`、CI のログのように出力をどこかへ流したときも、ページャは自動的に使いません。`--json` は常に標準出力にそのまま書きます。

既定では、`heal status` はキャッシュ済みの TODO が新しければそれを使い回すので、2 回目以降はほとんど時間がかかりません。キャッシュがない、または古いときは、自動で走査し直して新しいものに置き換えます。キャッシュが新しくても走査し直したいときは、`--refresh` を使います。

キャッシュが新しいかどうかは、HEAD と、作業ツリーに未コミットの変更がないかに加えて、有効にしている git 以外の入力でも判断します。git の管理外にある LCOV レポートやドキュメントのペアのファイルを更新すれば、HEAD が動いていなくてもキャッシュは無効になります。ファイルの更新時刻や、チェックアウトした場所の絶対パスは判断に使いません。人向けの出力でも JSON でも、カバレッジの観測状態を `missing`、`read_error`、`partial`、`complete` で区別します。レポートに載っていない本番ファイルは「未計測」として扱い、リポータやパッケージの範囲を確かめるよう促します。計測済みの 0% としては扱いません。

出力では、Finding を有効な解消 Tier と Severity ごとにまとめ（優先度の低いセクションは `--all` を付けたときだけ表示します）、ファイルごとに 1 行に集約します。Hotspot は、すべてが Hotspot のセクションや、Hotspot とそうでないものが混ざった行に `🔥` として表示されます。並び順は Tier、Severity、ファミリ内の `hotspot_score` の降順で、同点ならメトリクス、パス、id の順に決めます。Code・Test・Docs のスコアを互いに比べることはありません。スコアは確率でも、修正の効果を約束するものでもありません。

`--severity` は常に下限として働きます。`--all` は、その下限以上で隠れていたセクションを表示しますが、下限より下の Finding を戻すことはありません。

## `heal diff`

今の作業ツリーを、以前のコミット時点の Finding と比べます。既定の比較先は calibration の基準点の SHA（`heal init` や `heal calibrate --force` が記録します）で、記録がなければ `HEAD` を使います。そのため「Progress: N% complete」は、そのまま「calibration 以降にどれだけ片付いたか」と読めます。

```sh
heal diff                              # 今の状態と calibration の基準点を比べる
heal diff HEAD                         # 直前のコミットと比べる
heal diff main                         # main と比べる
heal diff v0.2.1                       # v0.2.1 タグと比べる
heal diff HEAD~5                       # 5 つ前のコミットと比べる
heal diff --all                        # Improved、Unchanged、High 未満の行も表示する
heal diff --hide-accepted              # `heal mark accept` で受け入れ済みの行を隠す
heal diff --no-pager                   # ページャを通さず標準出力に書く
heal diff --json                       # 機械可読な形で出力する
```

`<git-ref>` には、`git rev-parse` が理解できるものなら何でも渡せます。heal は指定されたコミットを*今の* `config.toml` と `calibration.toml` で評価し直すので、同じ条件で比べられます。見えるのは当時の評価ではなく、今のルールで過去と現在を判定した結果です。

標準出力が端末のときは、`heal diff` も `heal status` と同じく `$PAGER`（なければ `less`）を通します。`--no-pager` を付けると標準出力にそのまま書き、`--json` は常にそのまま書きます。

出力は Resolved / Regressed / Improved / New / Unchanged の各グループと、進み具合のパーセンテージです。比較の右側は常に、今の作業ツリーをその場で走査した結果で、保存はしません。

人向けの表示では、比較前と比較後の Severity がどちらも High 未満の行を既定で隠し、`[N entries below High hidden — pass --all]` というフッターを出します。隠さないと、ノイズの多い基準点のせいで対処すべき行が埋もれるからです。`--all` はこの絞り込みを外し、Improved / Unchanged のグループも表示します。`--json` の出力はどちらの場合も絞り込まないので、スキルや CI はすべての行を受け取ります。

チームが `heal mark accept` で受け入れた Finding には `📌 accepted` の印が付くので、New や Regressed の行でも「把握済みで対応不要」とひと目で分かります。`--hide-accepted` を付けるとこれらの行を消し、対処が必要な行だけを見られます。件数は `[N accepted entries hidden]` というフッターに残ります。2 つの絞り込みは独立していて、`--all --hide-accepted` ならすべての Severity を表示しつつ、受け入れ済みの行は省きます。

カバレッジを有効にしていると、JSON に `from_coverage_observation` と `to_coverage_observation` が入ります。それぞれの `missing` / `read_error` / `partial` / `complete` の状態と出どころの一覧によって、計測していない側を、計測済みの 0% や 100% と取り違えずに済みます。

受け入れ済みの Finding でも、Severity が上がったときや、そのファミリの Hotspot が false から true に変わったときは、見直しを促す通知が出ます。通知を出すのは status、diff の現在側、post-commit フックです。JSON では、理由を 1 つか 2 つ持つ `accepted_rereview` のエントリが 1 つ返ります。この通知で受け入れが取り消されたり、Finding が解消キューに戻ったりはしません。

とても大きなリポジトリでは、比較に時間がかかることがあります。`config.toml` の `[diff]` で LOC の上限を決めておくと、それを超えたときに 2 つのブランチを手動で比べる手順へ切り替わります。[Code › 設定](/heal/ja/code/configuration/#diff) を参照してください。

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

有効なメトリクスすべての集計を表示します。主な言語、複雑な関数のワースト N 件、上位の Hotspot、最も分かれたクラスなどです。1 つのオブザーバの出力に絞るには `--metric <name>` を使います。指定できる名前は次のとおりです。

- **Code**（いつでも使える）: `loc`、`complexity`、`churn`、`change-coupling`、`duplication`、`hotspot`、`lcom`
- **`[features.docs]`**（有効なとき）: `doc-freshness`、`doc-drift`、`doc-coverage`、`doc-link-health`、`orphan-pages`、`todo-density`、`doc-hotspot`
- **`[features.test]`**（有効なとき）: `coverage-pct`、`skip-ratio`、`test-hotspot`

`--json` は同じ内容を機械可読な JSON で出力するので、`jq` に渡すのに向いています。

標準出力が端末のときは、`heal metrics` も `heal status` / `heal diff` と同じく `$PAGER`（なければ `less`）を通します。`--no-pager` を付けると、標準出力にそのまま書きます。

実行のたびに最初から計算し直します。差分を取るための過去の記録はありません。

## `heal calibrate`

```sh
heal calibrate            # calibration.toml がなければ作る。あれば何もしない
heal calibrate --force    # 常に走査し直して calibration.toml を上書きする
```

heal が自動で calibration をやり直すことは**ありません**。リファクタでコードベースが本当に良くなったときに、基準がこっそり動いてしまっては困るからです。`--force` を付けて実行するのは、次のようなときです。

- 大きな構造の変更で分布が変わったとき（`heal doctor` が知らせ、`/heal:setup` がやり直しを勧めます）。
- `config.toml` で `floor_critical` / `floor_ok` の上書きを変え、それに合わせてパーセンタイルの段階を作り直したいとき。

生成される `calibration.toml` の先頭には出どころを示すコメントが入るので、ファイルを開いた人はこのコマンドにたどり着けます。`floor_critical` / `floor_ok` の上書きは、`calibration.toml` ではなく `config.toml` に書いてください。そうすれば `heal calibrate --force` で消えずに済みます。

`[features.semantic]` を有効にしていると、`heal status --focus <file>` で、ファイルに書いた作業に合わせた並び順を出せます（[Semantic (Jev)](/heal/ja/semantic/) を参照）。このときは常に走査し直し、保存済みの TODO リストは更新しません。

## `heal semantic ask`

`[features.semantic] enabled = true` のときだけ使えます。何が送られるかは [Semantic (Jev)](/heal/ja/semantic/) を参照してください。

```sh
heal semantic ask --dry-run          # 計画と料金の見積もりだけ。何も送らない
heal semantic ask                    # 有効なタスクすべてを問い合わせる
heal semantic ask --task <id>        # 1 つのタスクだけ（繰り返し指定できる）
heal semantic ask --refresh          # 答えが保存済みでも問い合わせ直す
heal semantic ask --prune            # どこからも参照されない保存済みの答えを消す
heal semantic ask --check            # キーが API で通るかだけを確かめる
heal semantic ask --task focus --focus plan.md    # plan.md に書いた作業に合わせて並べる
heal semantic ask --task verify_patch --diff HEAD~1..HEAD   # コミット範囲を判定する
heal semantic ask --json             # 実行結果を機械可読な形で出力する
```

終了コード `2` は、利用者の側でしか直せない問題を表します。機能が無効になっている、API キーが設定されていない、キーが拒否された、設定した `model` を API が知らない、のどれかです。

## `heal auth jev`

```sh
printf '%s\n' "$KEY" | heal auth jev set   # ユーザー設定に保存する（mode 600）
heal auth jev status                        # キーの出どころと、API での確認結果
heal auth jev status --offline              # API での確認を省く
heal auth jev clear                         # 保存したキーを消す
```

`TYPESAFE_API_KEY`（または `TYPESAFEAI_API_KEY`）が設定されていれば、保存したキーより優先されます。キーが `.heal/` の下に書かれることはありません。

## キャッシュを覗く

スクリプトから使うなら、`heal status --json` が正式な窓口です。ディスク上の状態を直接覗きたいときは、`.heal/findings/` の下に 4 つのファイルがあります。

| ファイル                         | 役割                                                                             |
| -------------------------------- | -------------------------------------------------------------------------------- |
| `.heal/findings/latest.json`     | 今の TODO。新しければ使い回し、古い・ない・`--refresh` のときは置き換える。      |
| `.heal/findings/fixed.json`      | スキルが `heal mark fix` で記録した修正の一覧（件数に上限がある）。              |
| `.heal/findings/accepted.json`   | チームが `heal mark accept` で受け入れた Finding（直さないもの、本質的なもの）。 |
| `.heal/findings/regressed.jsonl` | 直したはずの Finding が再び見つかったときの監査記録。                            |

2 つの JSON は、あえてバイト単位では一致させていません。`latest.json` はオブザーバの生の記録です。`heal status --json` は同じ構造を使いつつ、今の受け入れ状態と、その場限りの `accepted_rereview` 通知を重ねます。さらに、指定された workspace、feature、metric、path、Severity の絞り込みを、Finding、見直しの通知、それらの集計に当てます。カバレッジの観測状態そのものには Severity がありません。workspace とパスの範囲には従い、Test 以外のファミリや coverage 以外のメトリクスを指定したときは省きます。

どれも普通のファイルなので、`jq` で読めます。

```sh
jq '.severity_counts' .heal/findings/latest.json
jq 'keys | length' .heal/findings/fixed.json     # 記録済みの修正の件数
tail .heal/findings/regressed.jsonl
```

## `heal hook commit`

`heal init` が設置した git の post-commit フックから、自動で呼ばれます。すべてのオブザーバを実行し、Severity の通知を出します。`Critical` と `High` の Finding をすべて標準出力に書き、Hotspot の付いたものを先に並べます。一度表示した問題をしばらく黙らせる仕組みはなく、直すまでコミットのたびに表示されます。それがこの通知の狙いです。ディスクには何も書かず、出力はこの通知だけです。

`[features.test.coverage]` を有効にしていると、通知に 2 行目が加わることがあります。High / Critical の `coverage_pct` の Finding が Hotspot のファイルにあるときに、その件数を「uncovered hotspot」として数える行です。次のテストをどこに書けばいいかを、いちばん短く伝える行です。

カバレッジがない、読めない、一部しかないときは、status と同じく、リポータやパッケージの範囲を確かめるよう案内します。計測していないファイルを、テストのない Hotspot と見なすことはしません。受け入れ済みの項目のうち、判断の前提を見直す必要が出たものも知らせます。

手で実行すると、デバッグに役立つことがあります。

```sh
heal hook commit
```

## ヒント

- **基本の流れは `heal status` です。** 意味のあるコミットをしたら実行して、キャッシュを新しくし、TODO リストに何が残っているかを確かめてください。
- **`heal diff`**（引数なし）は、calibration の基準点からの進み具合を見せます。「% complete」は「calibration 以降にどれだけ片付いたか」と読めます。直前のコミット以降を見たいなら `HEAD` を渡します。ほかにも、`git rev-parse` が理解できる参照なら何でも渡せます。
- **post-commit フックは残しておいてください。** 消すと、コミットのたびの Severity の通知が止まります。`heal status` は、それでも必要なときにいつでも使えます。
