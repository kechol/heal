---
title: Code · スキル
description: 常時オンの Code ファミリ向けの、heal Claude Code プラグインのスキル — /heal:setup と /heal:refactor。
---

heal のスキルは Claude Code プラグインとして配布していて、heal が集めた Finding がそのまま Claude のセッションに流れます。Claude Code で一度だけ入れておきます。

```
/plugin marketplace add kechol/heal
/plugin install heal@heal
```

プラグインはプロジェクトではなく Claude Code の設定側に入るので、git 管理下のファイルは増えません。プラグインは公開時の heal リリースに固定されています。`heal` CLI を上げたらプラグインも更新してください(`/plugin marketplace update heal` のあと `/plugin update heal@heal`)。heal を使っているプロジェクトでセッションを始めると、CLI とプラグインのリリースが違う場合にだけ 1 行の案内が出ます。

プラグインのスキルは 4 つです。

| スキル           | 用途                                                                       |
| ---------------- | -------------------------------------------------------------------------- |
| `/heal:setup`    | heal のセットアップと、その確認。このページで説明します。                  |
| `/heal:refactor` | コードの Finding を読んでリファクタを提案し、承認されたものを適用する。    |
| `/heal:docs`     | 同じことをドキュメントに — [Docs › スキル](/heal/ja/docs/skills/) を参照。 |
| `/heal:tests`    | 同じことをテストに — [Test › スキル](/heal/ja/test/skills/) を参照。       |

どのスキルも、提案をあなたが承認するまではファイルを変更せず、push や pull request の作成もしません。

## `/heal:setup` — セットアップと確認

いつ実行しても安全です。まず `heal doctor` で何が済んでいるかを確かめ、足りないものや古くなったものだけを片付けます。

- **初期化** — `.heal/` がなければ `heal init` を実行します。
- **厳しさの調整** — Strict / Default / Lenient のどれにするかを聞き、コードベースを調べた結果(除外するパス、モノレポの workspace、シグナルの出ないメトリクス)をもとに `.heal/config.toml` を書きます。初回と、調整を頼んだときに提案します。
- **再 calibrate** — calibration 後に 200 を超えるコミットが入った、ファイル数が 20% を超えて変わった、修正を 10 件以上記録して Critical / High が残っていない、のどれかのとき。必ず先に確認します。heal が自分で calibrate し直すことはありません。
- **オプションのファミリを有効にする** — それぞれに必要なものを用意します。Docs には doc pairs、Test にはカバレッジのリポータ、[Semantic (Jev)](/heal/ja/semantic/) には API キーと concept の一覧です。
- **古いスキルのフォルダを消す** — heal 0.6 以前がプロジェクトにコピーしたもの(`.claude/skills/heal-*`、`.agents/skills/heal-*`)。

セットアップ済みのプロジェクトなら、そろっていると報告して何も聞かずに終わります。手順を指定すると直接そこへ進みます: `/heal:setup docs`、`/heal:setup tests`、`/heal:setup semantic`、`/heal:setup config`。

トリガーフレーズ: 「set up heal」、「check my heal setup」、「make heal stricter」、「enable heal coverage」、「/heal:setup」。

## `/heal:refactor` — リファクタの提案と適用

heal がコードに見つけたものを理解し、手を打つまでを 1 つのスキルで行います。5 つの手順で進みます。

1. **診断** — `heal status --all --feature code --json` を読み、指摘されたファイルを開いて、システムとして読みます。複数の Finding を抱えるファイル、モジュールの境界をまたいで一緒に変わるファイルの組、ハブになっているファイル、コードベースの層の分け方を把握します。
2. **提案** — 最初に短いアーキテクチャの読み(いちばん大きい問題が複雑さ・重複・結合・概念の混在のどれか)を示し、続けて番号付きの提案を出します。Hotspot ファイル上の Critical な Finding が先です。各提案には、変更の中身、取り除く摩擦、解消する Finding、触るファイル、リスクを付けます。
3. **選択** — どの提案を適用するかをあなたが選びます。
4. **適用** — 1 提案につき 1 コミット。コミットの前にビルドとテストを実行し、失敗したらその試みは捨てます。コミットのあとで解消した Finding を記録し(`heal mark fix`)、`heal status` で確かめます。
5. **報告** — 適用したもの、見送ったものとその理由、キューに残っているものをまとめます。

**構造的な提案** — ファイルを概念ごとに分ける、関数を本来のモジュールに移す、クラスを抜き出す、層の境界にインターフェースを置く、といった変更も、独立した 2 つ以上のシグナルが同じ境界を指していれば通常の提案として出します。たとえば LCOM のクラスタが [Semantic (Jev)](/heal/ja/semantic/) の見つけた概念の分かれ目と一致する場合や、関数の概念が別のファイルにあって、しかもそのファイルと一緒に変更されている場合です。シグナルが 1 つだけなら、提案ではなく質問として挙げます。

**判断が要るもの** — 公開 API の改名(公開している項目、CLI フラグ、JSON のフィールド)、どちらも成り立つ 2 つのモジュール境界からの選択、ドメイン単位の組み替えは、適用の前に具体的な選択肢を付けて個別に確認します。

**直さずに受け入れる** — Finding の中には意図したものを数えているだけのものがあります。生成されたパーサのテーブル、閉じた enum に対する網羅的な `match`、手順がひとまとまりのパイプラインなどです。こうしたものは短い理由を添えて受け入れ済みとして記録する(`heal mark accept`)よう提案し、キューから外します。あなたの承認なしに受け入れることはありません。

**[Semantic (Jev)](/heal/ja/semantic/) を有効にしている場合**は、提案を見せる前に確かめ(`verify_proposal`)、コミットのあとにも確かめます(`verify_patch`)。複雑さが移っただけ、条件を早期 return に反転しただけ、コミットメッセージが変更と合わない、リファクタのつもりで振る舞いが変わった、と判定されたら、そのコミットを取り消して次へ進みます。機能が無効か API キーがなければ、これらの確認は飛ばします。

引数: パス(`/heal:refactor src/payments`)を渡すとその下の Finding に絞り、Finding の id を渡すとその Finding に絞り、`plan` を渡すと提案までで止まります。

トリガーフレーズ: 「what does heal say?」、「where should we refactor?」、「fix the heal findings」、「/heal:refactor」。

## heal 0.6 以前からのアップグレード

以前のバージョンは 12 個のスキルを CLI に同梱し、各プロジェクトにコピーしていました。プラグインでは次のように対応します。

| 以前                                                                                       | 現在               |
| ------------------------------------------------------------------------------------------ | ------------------ |
| `/heal-setup`、`/heal-concepts-setup`、`/heal-doc-pair-setup`、`/heal-test-reporter-setup` | `/heal:setup`      |
| `/heal-code-review`、`/heal-code-patch`                                                    | `/heal:refactor`   |
| `/heal-doc-review`、`/heal-doc-patch`、`/heal-doc-scaffold`                                | `/heal:docs`       |
| `/heal-test-review`、`/heal-test-patch`                                                    | `/heal:tests`      |
| `/heal-cli`                                                                                | (プラグインに内蔵) |

プラグインを入れたら、各プロジェクトで `heal skills uninstall` を実行して古いコピーを消し、削除をコミットしてください。消すのは heal が書いたフォルダだけで、自分で作ったスキルは残ります。
