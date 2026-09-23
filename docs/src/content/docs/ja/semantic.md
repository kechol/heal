---
title: Semantic (Jev)
description: TypeSafe の分類モデル Jev による判定（opt-in）。何が送られるか、API キーの設定、答えの保存のしかた。
---

HEAL のメトリクスは、ソースと git の履歴からローカルで計算します。
どこが変えにくいかは分かりますが、**コードが何を意味しているか**までは分かりません。
たとえば、関数の名前が中身と合っているか、テストが本当に何かを確かめているか、
ドキュメントの説明がまだコードと合っているか、といったことです。

opt-in の `[features.semantic]` は、こうした問いを TypeSafe の分類モデル
[Jev](https://docs.typesafe.ai/) に投げます。Jev は文章やコードを書きません。
問いごとに確率で答えるので、HEAL はほかのメトリクスと同じように、
その答えを Finding にできます。

この機能は**既定では無効**です。有効にしなければ、HEAL の動きは今までと変わりません。

## 何が、いつ送られるか

- 送信するのは `heal semantic ask` だけです。HEAL があらかじめ選んだコード・テスト・
  ドキュメントの一部（よく変更されるファイルの関数など）を TypeSafe の API に送ります。
- `heal auth jev status` は API キーが使えるかを確かめます。キーは送りますが、
  プロジェクトの中身は送りません。
- それ以外のコマンド（`heal status`、`heal metrics`、`heal diff`、post-commit hook）は
  ネットワークにつながりません。`heal semantic ask` が保存した答えを読むだけです。
- `[features.semantic].exclude` に一致するファイルは送りません。

TypeSafe は、利用者のデータをモデルの学習に使わないとしています。データを保持しない契約
（ZDR）は enterprise プランだけです。非公開のコードベースで有効にする前に、
[TypeSafe の規約](https://docs.typesafe.ai/legal.md)を確認してください。

## 有効にする

有効にするかどうかはチームで決めることなので、共有の設定ファイルに書きます。

```toml
[features.semantic]
enabled = true
# model = "jev-1.13.0"   # バージョンを固定する。動くエイリアスは使えない
# max_usd = 1.0          # 1 回の実行でこれ以上使う前に止める
# exclude = ["secrets/", "*.pem"]
```

有効にすると、`heal status` は保存された答えを使って TODO リストを並べ替えます。
同梱の patch 系スキルは、自分の変更を Jev に確かめさせます。

## API キーを設定する

キーは一人ひとりが自分のものを使います。
[TypeSafe のコンソール](https://console.typesafe.ai/)で取得したら、環境変数に入れるか、

```sh
export TYPESAFE_API_KEY=...
```

ユーザーの設定ファイル（プロジェクトの外にあり、自分だけが読める）に保存します。

```sh
printf '%s\n' "$KEY" | heal auth jev set
heal auth jev status    # キーの出どころを表示し、使えるか確かめる
heal auth jev clear     # 保存したキーを消す
```

キーが `.heal/` の下に書かれることはありません。環境変数 `TYPESAFE_API_KEY` と
保存したファイルの両方がある場合は、環境変数が優先されます。`TYPESAFEAI_API_KEY`
も読むので、ほかの Jev 向けツールで使っているキーをそのまま使えます。

## 問い合わせる

```sh
heal semantic ask --dry-run   # 何を問うかと、見積もり金額だけを表示する
heal semantic ask             # 問い合わせて、答えを保存する
heal semantic ask --prune     # どこからも参照されなくなった答えも消す
```

答えは `.heal/semantic/verdicts/` に、タスクごとに 1 ファイルで保存されます。
このディレクトリは commit してください。チームメイトは API キーがなくても同じ結果を見られ、
HEAL は前回から変わったコードだけを問い合わせます。
2 つのブランチがそれぞれ答えを追加したときの衝突は、次の設定で減らせます。

```text
# .gitattributes
.heal/semantic/verdicts/*.jsonl merge=union
```

## 料金

Jev の料金は入力の分だけです（執筆時点で、入力 10 億 tokens あたり $42）。
`--dry-run` は送る前に見積もりを表示し、`max_usd` は上限を超える前に実行を止めます。
中規模のリポジトリなら、1 回の実行は数セント程度です。

## 結果は候補として扱う

分類モデルは間違えることがあり、しきい値に近いスコアはモデルのバージョンによって少し変わります。
HEAL はモデルのバージョンを固定し、保存した答えを使い回すので結果は安定しますが、
semantic の Finding は、人が見て判断する候補として扱ってください。

## 無効にする

`enabled = false` にします。次の `heal status` で semantic の Finding はすべて消え、
並び順も元に戻ります。保存した答えは、消すまで `.heal/semantic/` に残ります。
