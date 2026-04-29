---
title: アーキテクチャ
description: heal がどこにデータを置き、いつ何が書き出されるか、各要素がどう組み合わさるか。
---

このページでは、heal がどんなファイルを作り、いつ書き出され、何を
含むかを説明します。ナッジが出ない原因を調べたり、JSON 出力に対し
てスクリプトを書いたり、heal がバックグラウンドで何をしているのか
を理解したいときに役立ちます。

## 全体像

```
git commit
    │
    ▼
.git/hooks/post-commit  ──►  heal hook commit
                                  │
                                  ├──►  オブザーバー（LOC, complexity, churn, …）
                                  │
                                  ├──►  .heal/snapshots/YYYY-MM.jsonl
                                  │
                                  └──►  .heal/logs/YYYY-MM.jsonl

claude セッション開始
    │
    ▼
SessionStart フック  ──►  heal hook session-start
                              │
                              ├──►  最新スナップショット + 差分を読む
                              │
                              ├──►  .heal/state.json を読む（クールダウン）
                              │
                              └──►  Markdown ナッジを stdout に出力（Claude が読む）
```

`heal` は単一バイナリです。両方の経路がこれを通ります。デーモンも、
スケジューラも、バックグラウンドプロセスもありません。

## オンディスクのレイアウト

`heal init` 直後:

```
<your-repo>/
├── .heal/
│   ├── config.toml                # 自分で編集する。heal init がデフォルトを書く
│   ├── snapshots/
│   │   └── 2026-04.jsonl          # コミットごとの完全メトリクススナップショット
│   ├── logs/
│   │   └── 2026-04.jsonl          # 軽量なイベントタイムライン
│   └── state.json                 # ナッジルールのクールダウンタイムスタンプ
│
├── .git/hooks/post-commit         # `heal hook commit` を呼ぶ 1 行のシム
│
└── .claude/plugins/heal/          # Claude プラグイン（`heal skills install` 後）
```

## 何がいつ書かれるか

| ファイル                        | 書き出し元                  | タイミング                              |
| ------------------------------- | --------------------------- | --------------------------------------- |
| `.heal/config.toml`             | `heal init`                 | セットアップ時に一度。自由に編集可。    |
| `.heal/snapshots/YYYY-MM.jsonl` | post-commit フック          | `git commit` のたび。                   |
| `.heal/logs/YYYY-MM.jsonl`      | post-commit + Claude フック | コミットおよび Claude イベントのたび。  |
| `.heal/state.json`              | SessionStart フック         | ルールが発火するたびに更新。            |
| `.claude/plugins/heal/`         | `heal skills install`       | 一度だけ。`heal skills update` で更新。 |

## イベントログ

`snapshots/` と `logs/` はオンディスク形式が共通です:

- **月ごとに 1 ファイル**: `YYYY-MM.jsonl`（UTC）。
- **追記専用**: 各レコードは 1 行 1 つの JSON オブジェクト。

すべてのレコードは外側の形が同じです:

```json
{
  "timestamp": "2026-04-29T05:14:22Z",
  "event": "commit",
  "data": {
    /* … 形は event に依存 … */
  }
}
```

`event` フィールドが `data` のペイロード種別を示します。

### `snapshots/` — メトリクスペイロード

コミットごとに書き出されます。有効化されている全オブザーバーから
の出力をすべて含みます。`heal status` が読むのはこちらです。

```json
{
  "version": 1,
  "git_sha": "a0a6d1a…",
  "loc": {
    /* LocReport */
  },
  "complexity": {
    /* 無効ならば null */
  },
  "churn": {
    /* … */
  },
  "change_coupling": null,
  "duplication": {
    /* … */
  },
  "hotspot": {
    /* … */
  },
  "delta": {
    /* SnapshotDelta、または初回スナップショットでは null */
  }
}
```

`delta` は前回のスナップショットからの変化をまとめます — worst-N
リストの新規エントリ、`max_ccn` の変化、Hotspot ランキングのシフト
など。SessionStart のナッジはこれを参照してどのルールを発火させる
かを決定します。

### `logs/` — イベントタイムライン

コミットおよび Claude フックイベントごとに書き出される軽量レコード
です。`heal logs` が読むのはこちらです。

| イベント種別    | 書き出されるタイミング                                |
| --------------- | ----------------------------------------------------- |
| `init`          | `heal init` 実行時                                    |
| `commit`        | `git commit` 着地時（コミットメタデータのみ）         |
| `edit`          | Claude がファイルを編集したとき（PostToolUse フック） |
| `stop`          | Claude のターンが終わったとき（Stop フック）          |
| `session-start` | Claude セッションが開いたとき（SessionStart フック）  |

`logs/` の `commit` イベントは軽量メタデータ（sha、author、メッセー
ジサマリ、変更ファイル）のみを保持します — メトリクスペイロード本
体は持ちません。この分離により、有効化されているメトリクス数に関わ
らずタイムラインクエリが速いままです。

どちらのストリームも標準的な Unix ツールで直接調査できます:

```sh
# 最新 5 件のコミットイベント
heal logs --filter commit --limit 5

# スクリプティング用の生 JSON
heal logs --json | jq '.data.git_sha'
```

## ステート

`.heal/state.json` は、同じナッジが毎セッションに出ないよ
うクールダウンのタイムスタンプを追跡します。

```json
{
  "last_fired": {
    "complexity.spike": "2026-04-28T03:14:22Z",
    "hotspot.new_top": "2026-04-25T11:02:08Z"
  }
}
```

ルールが発火すると、heal はそのタイムスタンプをここに記録します。
次の SessionStart は `cooldown_hours` が経過するまでそのルールを抑
制します。書き込みはアトミック（一時ファイルに書いてからリネーム）
なので、プロセスが中断されてもファイルが半端な状態で残ることはあり
ません。
