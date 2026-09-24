---
title: Semantic (Jev)
description: TypeSafe の分類モデル Jev による判定（opt-in）。何が送られるか、API キーの設定、答えの保存のしかた。
---

heal のメトリクスは、ソースと git の履歴からローカルで計算します。どこが変えにくいかは分かりますが、**コードが何を意味しているか**までは分かりません。たとえば、関数の名前が中身と合っているか、テストが本当に何かを確かめているか、ドキュメントの説明がまだコードと合っているか、といったことです。

opt-in の `[features.semantic]` は、こうした問いを TypeSafe の分類モデル [Jev](https://docs.typesafe.ai/) に投げます。Jev は文章やコードを書きません。問いごとに確率で答えるので、heal はほかのメトリクスと同じように、その答えを Finding にできます。

この機能は**既定では無効**です。有効にしなければ、heal の動きは今までと変わりません。

## 何が、いつ送られるか

- 送信するのは `heal semantic ask` だけです。heal があらかじめ選んだコード・テスト・ドキュメントの一部（よく変更されるファイルの関数など）を TypeSafe の API に送ります。
- `heal auth jev status` は API キーが使えるかを確かめます。キーは送りますが、プロジェクトの中身は送りません。
- それ以外のコマンド（`heal status`、`heal metrics`、`heal diff`、post-commit hook）はネットワークにつながりません。`heal semantic ask` が保存した答えを読むだけです。
- `[features.semantic].exclude` に一致するファイルは送りません。

TypeSafe は、利用者のデータをモデルの学習に使わないとしています。データを保持しない契約（ZDR）は enterprise プランだけです。非公開のコードベースで有効にする前に、[TypeSafe の規約](https://docs.typesafe.ai/legal.md)を確認してください。

## 有効にする

有効にするかどうかはチームで決めることなので、共有の設定ファイルに書きます。

```toml
[features.semantic]
enabled = true
# model = "jev-1.13.0"   # バージョンを固定する。動くエイリアスは使えない
# max_usd = 1.0          # 1 回の実行でこれ以上使う前に止める
# concurrency = 8        # 同時に送るリクエストの数
# exclude = ["secrets/", "*.pem"]
```

有効にすると、`heal status` は保存された答えを使って TODO リストを並べ替えます。同梱の patch 系スキルは、自分の変更を Jev に確かめさせます。

## API キーを設定する

キーは一人ひとりが自分のものを使います。[TypeSafe のコンソール](https://console.typesafe.ai/)で取得したら、環境変数に入れるか、

```sh
export TYPESAFE_API_KEY=...
```

ユーザーの設定ファイル（プロジェクトの外にあり、自分だけが読める）に保存します。

```sh
printf '%s\n' "$KEY" | heal auth jev set
heal auth jev status    # キーの出どころを表示し、使えるか確かめる
heal auth jev clear     # 保存したキーを消す
```

キーが `.heal/` の下に書かれることはありません。環境変数 `TYPESAFE_API_KEY` と保存したファイルの両方がある場合は、環境変数が優先されます。`TYPESAFEAI_API_KEY` も読むので、ほかの Jev 向けツールで使っているキーをそのまま使えます。

## 問い合わせる

```sh
heal semantic ask --dry-run   # 何を問うかと、見積もり金額だけを表示する
heal semantic ask             # 問い合わせて、答えを保存する
heal semantic ask --prune     # どこからも参照されなくなった答えも消す
```

答えは `.heal/semantic/verdicts/` に、タスクごとに 1 ファイルで保存されます。このディレクトリは commit してください。チームメイトは API キーがなくても同じ結果を見られ、heal は前回から変わったコードだけを問い合わせます。2 つのブランチがそれぞれ答えを追加したときの衝突は、次の設定で減らせます。

```text
# .gitattributes
.heal/semantic/verdicts/*.jsonl merge=union
```

後述の確認用タスクと `focus` の答えは、一人ひとりの作業中の内容についての答えなので、代わりに `.heal/cache/semantic/verdicts/` に保存します。このディレクトリは git の対象外で、消しても問題ありません。

## HEAL が問うこと

問いの種類を「タスク」と呼びます。`[features.semantic.tasks.<id>] enabled = false` で個別に止められ、`cutoff = 0.7` のように、Finding にするのに必要な確率を変えられます。

| タスク               | 問うこと                                                                                                       | どこに現れるか                                                             |
| -------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `commit_intent`      | 最近の commit が、バグ修正・機能追加・リファクタなどのどれに当たるか                                           | バグ修正が集中するファイルが、`heal status` の上のほうに来る               |
| `consequence`        | フラグの立ったファイルに不具合があったとき、どれだけの損失になるか                                             | 重要なコードが `heal status` の上のほうに来る                              |
| `triage`             | drain queue の Finding が、機械的に直せるか、誤検出か、設計判断が要るか、修正の大きさ                          | patch 系スキルの判断材料。小さい修正から先に並ぶ                           |
| `friction`           | 複雑なコードが、変えにくいか・テストしにくいか・読みにくいか、複雑さが本質的か                                 | 実際に困るコードが上に来る                                                 |
| `focus`              | これからやる作業（`--focus`）が、各ファイルにどれだけ関わるか                                                  | `heal status --focus` で、先に整えるべきファイルが上に来る                 |
| `concept`            | 各関数が、語彙のどの概念を実装しているか（インラインの単体テストは除く）                                       | 概念が混ざったファイル、置き場所の違う関数、多くのファイルに散らばった概念 |
| `term_drift`         | 関数名の 2 つの単語が同じものを指しているか（`user` / `account`）                                              | 1 つのものに 1 つの単語を使うための rename の提案                          |
| `name_mismatch`      | 関数の名前や doc comment が、本体のしていることと合っているか                                                  | 中身と違うことを約束している名前                                           |
| `split_points`       | 長く複雑な関数が、1 つずつ目的を持つ手順にどこで分かれるか                                                     | 複雑さの Finding に付く分割位置。`/heal-code-patch` が使う                 |
| `fix_pattern`        | 複雑さや重複の Finding に、どの定番のリファクタリングが合うか                                                  | `/heal-code-patch` の手がかりになる                                        |
| `test_value`         | 各テストが、名前が示す振る舞いの不具合を捕まえられるか。mock・フレームワーク・何も確かめていないだけではないか | 削除・書き直しの候補（`[features.test]` が必要）                           |
| `mock_scope`         | 各 mock が何を差し替えているか（外部のサービス、テスト対象そのもの、内部の部品）                               | テストを実装の詳細に縛り付けている mock                                    |
| `test_triage`        | カバーされていないコードが、ロジック・協調・I/O のどれか。skip されたテストの理由                              | `/heal-test-review` が、単体テストの効く場所を判断する材料                 |
| `test_duplicate`     | 似た 2 つのテストが同じことを確かめているか、入力が違うだけか                                                  | 削除するか、1 つのテーブル駆動テストにまとめる候補                         |
| `doc_structure`      | 各節がどの種類の文書か（tutorial、how-to、reference、explanation など）、どこから別の文書が始まるか            | 分割・統合すべきページ、種類が混ざったページ（`[features.docs]` が必要）   |
| `doc_placement`      | 読み手が各ページを探しそうな docs の節はどこか                                                                 | 置き場所の違うページと、孤立したページのリンク先                           |
| `doc_drift_semantic` | ペアの doc の節が、まだコードのしていることを説明しているか                                                    | コードがもうしていないことを書いている節                                   |
| `doc_concept`        | 各 doc の節が、語彙のどの概念を説明しているか                                                                  | コードが頼っているのに、どの doc も説明していない概念                      |
| `doc_overlap`        | 2 つのページが同じ概念を二重に説明していないか、食い違っていないか                                             | まとめるべき説明と、直すべき食い違い                                       |

### TODO リストの並び順

答えが保存されていても、`heal status` は今までどおり tier と severity でまとめます。そのうえで各まとまりの中を、不具合の損失が大きいファイル、扱いにくいコード、バグ修正が集中するファイル、手間の小さい修正の順に並べ、最後にいつもの hotspot の値で並べます。答えがなければ、並び順は今までと変わりません。同梱の patch 系スキルやスクリプトは、同じ並び順を `heal status --json` から読めます。キューに入る Finding にはそれぞれ `drain_rank`（1 が次に扱うもの。ファミリーごとに数える）と `drain_tier` が付きます。

特定の作業に備えたいときは、作業の内容をファイルに書いて次を実行します。

```sh
heal semantic ask --task focus --focus plan.md
heal status --focus plan.md
```

### 概念の語彙

`concept` タスクには、コードを形づくる概念の一覧が `.heal/concepts.toml` に必要です。Jev は渡された名前の中から選ぶだけで、名前を作ることはありません。同梱のスキルで下書きを作り、見直してから commit してください。

```sh
claude /heal-concepts-setup
```

```toml
[[concept]]
id = "calibration"
description = "Derives thresholds from the project's own metric distribution."
```

patch 系と review のスキルは、`--task` を付けて確認用のタスク（`verify_patch`、`verify_tests`、`verify_proposal`、`name_choice`）も実行します。`verify_patch` と `verify_tests` は、`--diff` で渡した commit（例: `--diff HEAD~1..HEAD`）を判定します。どれもエージェント自身の作業を確かめるためのもので、`--task` なしの `heal semantic ask` では実行されません。

## 料金

Jev の料金は入力の分だけです（執筆時点で、入力 10 億 tokens あたり $42）。`--dry-run` は送る前に見積もりを表示し、`max_usd` は上限を超える前に実行を止めます。中規模のリポジトリなら、1 回の実行は数セント程度です。

## 結果は候補として扱う

分類モデルは間違えることがあり、しきい値に近いスコアはモデルのバージョンによって少し変わります。heal はモデルのバージョンを固定し、保存した答えを使い回すので結果は安定しますが、semantic の Finding は、人が見て判断する候補として扱ってください。

## 無効にする

`enabled = false` にします。次の `heal status` で semantic の Finding はすべて消え、並び順も元に戻ります。保存した答えは、消すまで `.heal/semantic/` に残ります。
