---
title: Test · スキル
description: '[features.test] 向けの heal プラグインのスキル — テストスイートを見直して改善する /heal:tests と、カバレッジを配線する /heal:setup の手順。'
---

オプトインの **Test** ファミリは、heal の Claude Code プラグインにある 2 つのスキルで扱います。ファミリを有効にしてカバレッジのリポータを配線する `/heal:setup` の tests の手順と、テストそのものに手を入れる `/heal:tests` です。プラグインの入れ方は [Code › スキル](/heal/ja/code/skills/) を参照してください。

## `/heal:setup tests` — ファミリを有効にして lcov を配線する

`[features.test]` を有効にし、`test_paths` と `lcov_paths` をリポジトリに合わせて設定します。続けて言語スタックを検出し、heal が読む場所に `lcov.info` が出るようにカバレッジのリポータを用意します。

| スタック | リポータ                                                                   |
| -------- | -------------------------------------------------------------------------- |
| Rust     | `cargo llvm-cov --lcov --output-path lcov.info`                            |
| Python   | `pytest --cov=src --cov-report=lcov`(`pytest-cov`)                         |
| JS / TS  | `nyc --reporter=lcov mocha` / `vitest --coverage --coverage.reporter=lcov` |
| Go       | `go test -coverprofile=coverage.out` + `gcov2lcov`                         |
| Scala    | `scoverage` プラグイン + lcov リポータ                                     |
| 混在     | スタックごとに 1 つずつ                                                    |

リポータのインストール、`.heal/config.toml` の編集、リポータの実行は、どれも実行前に確認します。`[features.test.coverage].post_commit_refresh` を設定して、post-commit フックでコミットのたびに `lcov.info` を更新することもできます。CI の変更はコピーして使える提案として表示するだけで、適用はしません。カバレッジを有効にしているのに lcov ファイルが無いときは `heal doctor` が test を `todo` と報告するので、`/heal:setup` がこの手順をもう一度提案します。

## `/heal:tests` — テストを見直して直す

テスト版の `/heal:refactor` です。test の Finding を読み、変更を提案し、あなたが承認したものだけを 1 提案 1 コミットで適用します。コミットの前には必ずテストスイートが通ることを確かめます。push や pull request の作成はしません。

1. **診断** — `heal status --all --feature test --json` を読み、指摘されたソースファイルをそのテストと一緒に読みます。テストの無いコードは、**テストピラミッド**に沿って、どの層でテストすべきかで分けます。純粋なロジックには単体テスト、他のモジュールを組み合わせるだけのコードには小さな結合テスト、I/O の境界にあるコードには薄い契約テストです。skip は理由(環境、遅い、壊れている、保留中)で分けます。
2. **提案** — スイートの形(逆ピラミッド、テストの無い Hotspot、たまっていく skip、ソースに追いついていないテスト)を短くまとめ、優先度の高い Finding から番号付きで提案します。テストを書く提案には、確かめるケースを添えます。
3. **選択** — どの提案を適用するかをあなたが選びます。
4. **適用** — 1 提案につき 1 コミット。スイートが通ることが条件で、カバレッジが変わるときはリポータを再実行します。
5. **報告** — 変えたものと、残っているものをまとめます。

Finding ごとの主な提案:

| メトリクス              | よくある提案                                                                                            |
| ----------------------- | ------------------------------------------------------------------------------------------------------- |
| `coverage_pct`          | 振る舞いが文書化された純粋なロジックに単体テスト。組み合わせや I/O のコードには結合テストか契約テスト。 |
| `change_coupling.drift` | テストをソースの今の契約に合わせる。振る舞いが別の場所に移ったなら書き直す。                            |
| `skip_ratio`            | skip の理由がもう成り立たないテストを戻す。機能が無くなったテストは消す。それ以外は確認する。           |

同じ問題が複数のファイルにまたがるときは、**スイート全体**の変更も提案します。逆ピラミッドの立て直し、スイートの分割、共通の fixture の切り出し、テストファイルの退役などです。期待する振る舞いがどこにも書かれていないコードは質問として挙げます — テストを書いても推測を固めるだけだからです。

**やらないこと:** テストを通すためにアサーションを弱めない、`skip_ratio` を下げるために flaky なテストを skip しない、実行していないテストをコミットしない、Finding を消すためにカバレッジのしきい値を下げたりファミリを無効にしたりしない。

**[Semantic (Jev)](/heal/ja/semantic/) を有効にしている場合**は、何も確かめていないテストも消せます。確信度の高い `delete` の付いた `test_value` の Finding を、1 コミットに 1 テストずつ、スイートが通りカバレッジが下がらない場合に限って消します(自分のモックしか確かめていないテストは例外)。重複したテスト(`test_duplicate`)はまとめるか消し、ただの値のモック(`mock_scope`)は本物の値に置き換えます。コミットのたびに `heal semantic ask --task verify_tests --diff HEAD~1..HEAD` を実行し、新しいテストが自分のモックしか確かめていなければそのコミットを取り消します。

引数: パスを渡すとその下の Finding に絞り、Finding の id を渡すとその Finding に絞り、`plan` を渡すと提案までで止まります。

トリガーフレーズ: 「review the test health」、「fix the test findings」、「add the tests heal flagged」、「remove useless tests」、「/heal:tests」。
