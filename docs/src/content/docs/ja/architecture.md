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
                                  ├──►  オブザーバー（LOC, complexity, churn, …, lcom）
                                  │       (run_all を 1 回; 結果は下流で再利用)
                                  │
                                  ├──►  .heal/snapshots/YYYY-MM.jsonl
                                  │       (MetricsSnapshot, severity_counts 含む)
                                  │
                                  ├──►  .heal/logs/YYYY-MM.jsonl
                                  │       (軽量な CommitInfo)
                                  │
                                  └──►  stdout: Severity ナッジ
                                         (Critical / High Finding のみ;
                                          トリガー発火時は recalibrate ヒントを先頭に追加)

ユーザー: heal check（または `claude /heal-code-patch`）
    │
    ▼
heal check  ──►  calibration.toml で Finding を分類
                       │
                       ├──►  .heal/checks/YYYY-MM.jsonl + latest.json
                       │       (CheckRecord — TODO リスト)
                       │
                       ├──►  fixed.jsonl ↔ regressed.jsonl を整合
                       │
                       └──►  Severity ごとのビューを stdout に描画
```

`heal` は単一バイナリです。両方の経路がこれを通ります。デーモンも、
スケジューラも、バックグラウンドプロセスもありません。post-commit
フックは全オブザーバーを **一度だけ** 実行し、その結果をスナップ
ショットライターとナッジの両方に渡します — オブザーバーが 1 コミッ
トあたり 2 回走ることはありません。

## オンディスクのレイアウト

`heal init` 直後:

```
<your-repo>/
├── .heal/
│   ├── config.toml                # 自分で編集する
│   ├── calibration.toml           # 自動 — heal init / heal calibrate
│   ├── snapshots/
│   │   └── 2026-04.jsonl          # MetricsSnapshot + CalibrationEvent ストリーム
│   ├── logs/
│   │   └── 2026-04.jsonl          # 軽量な commit / edit / stop イベント
│   └── checks/
│       ├── 2026-04.jsonl          # 追記専用の CheckRecord ストリーム
│       ├── latest.json            # 最新レコードのアトミックなミラー
│       ├── fixed.jsonl            # `/heal-code-patch` がコミットでの修正を主張
│       └── regressed.jsonl        # 修正済みが再検出された — 警告として表示
│
├── .git/hooks/post-commit         # `heal hook commit` を呼ぶ 1 行のシム
│
└── .claude/plugins/heal/          # Claude プラグイン（`heal skills install` 後）
```

## 何がいつ書かれるか

| ファイル / ディレクトリ         | 書き出し元                                   | タイミング                                        |
| ------------------------------- | -------------------------------------------- | ------------------------------------------------- |
| `.heal/config.toml`             | `heal init`                                  | セットアップ時に一度。自由に編集可。              |
| `.heal/calibration.toml`        | `heal init` / `heal calibrate`               | セットアップ時、その後は明示的な再 calibrate 時。 |
| `.heal/snapshots/YYYY-MM.jsonl` | post-commit フック + `heal calibrate`        | `git commit` ごと、再 calibrate ごと。            |
| `.heal/logs/YYYY-MM.jsonl`      | post-commit + Claude PostToolUse / Stop      | コミットおよび Claude ツールイベントごと。        |
| `.heal/checks/YYYY-MM.jsonl`    | `heal check`                                 | 新規 `heal check`（キャッシュミス経路）ごと。     |
| `.heal/checks/latest.json`      | `heal check`                                 | アトミックミラー；新規実行ごとにリフレッシュ。    |
| `.heal/checks/fixed.jsonl`      | `heal fix mark`（`/heal-code-patch` から呼出） | `/heal-code-patch` のコミット着地ごと。             |
| `.heal/checks/regressed.jsonl`  | `heal check`（整合パス）                     | 修正済み Finding が再検出されたとき。             |
| `.claude/plugins/heal/`         | `heal skills install`                        | 一度だけ。`heal skills update` で更新。           |

v0.2 以前の `.heal/state.json` は SessionStart ナッジとともに廃止
されました — 履歴状態の問い合わせは `EventLog::iter_segments` で
`snapshots/` を辿る方法に統一されています。

## イベントログ

`snapshots/`、`logs/`、`checks/` はオンディスク形式が共通です。

- **月ごとに 1 ファイル**: `YYYY-MM.jsonl`（UTC）。
- **追記専用**: 各レコードは 1 行 1 つの JSON オブジェクト。
- **透過的な gzip**: リーダーは `.gz` ファイルもプレーンテキストと
  並行して扱います。`heal compact`（`heal hook commit` から自動で
  も呼ばれます）が、90 日超のセグメントを gzip 圧縮し、365 日超を
  削除します。

すべてのレコードは外側の形が同じです。

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

コミットごと（`event = "commit"`）と再 calibrate ごと
（`event = "calibrate"`）に書き出されます。両者は同居しており、リー
ダーはデコード前に `event` でフィルタします。

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
  "change_coupling": {
    /* pairs[].direction = "symmetric" | "one_way" */
  },
  "duplication": {
    /* … */
  },
  "hotspot": {
    /* … */
  },
  "lcom": {
    /* classes[].cluster_count, clusters[].methods */
  },
  "severity_counts": { "critical": 2, "high": 5, "medium": 12, "ok": 84 },
  "codebase_files": 142,
  "delta": {
    /* SnapshotDelta、または初回スナップショットでは null */
  }
}
```

`delta` は前回スナップショットからの変化をまとめます。post-commit
ナッジはこれを参照しません — Severity は現在の `Finding` セットを
`calibration.toml` に照らして計算されます。

### `logs/` — イベントタイムライン

メトリクスペイロードを含まない軽量レコードです。`heal logs` が読み
ます。

| イベント種別 | 書き出されるタイミング                                |
| ------------ | ----------------------------------------------------- |
| `commit`     | `git commit` 着地時（CommitInfo メタデータ）          |
| `edit`       | Claude がファイルを編集したとき（PostToolUse フック） |
| `stop`       | Claude のターンが終わったとき（Stop フック）          |

`commit` イベントはメタデータのみ（sha、parent、author、メッセー
ジサマリ、変更ファイル）を保持します。フルなメトリクスペイロードは
`snapshots/` に残ります。この分離により、有効化されているメトリク
ス数に関わらずタイムラインクエリが速いままです。

### `checks/` — 結果キャッシュ

`/heal-code-patch` が消化する TODO リストです。`heal check` が唯一の
writer です。

```json
{
  "version": 1,
  "check_id": "01HKM3Q6Z1B7…",          // ULID
  "started_at": "2026-04-30T05:14:22Z",
  "head_sha": "a0a6d1a…",
  "worktree_clean": true,
  "config_hash": "9f8e7d6c5b4a3210",     // config + calibration の FNV-1a
  "severity_counts": { … },
  "findings": [ /* Vec<Finding> */ ]
}
```

`heal check` は `(head_sha, config_hash, worktree_clean=true)` が
最新キャッシュレコードと一致するときショートサーキットします — 同
じコミット上での再実行は無料です。

`fixed.jsonl` と `regressed.jsonl` は同じディレクトリにありますが、
`EventLog` のエンベロープではなくフラットな JSON-lines です。小さ
くて目的が単一の監査トレイルです。

```jsonl
{
  "finding_id": "ccn:src/payments/engine.ts:processOrder:9f8e…",
  "commit_sha": "a1b2c3",
  "fixed_at": "…"
}
```

新規 `heal check` で、既に fixed の `finding_id` が再出現すると、
エントリは `fixed.jsonl` から `regressed.jsonl` に移動し、レンダラー
が警告を出します。

これらのストリームは対応するブラウザコマンドで覗けます。

```sh
# logs/ 直近 5 件のコミットイベント
heal logs --filter commit --limit 5

# MetricsSnapshot + calibrate イベント
heal snapshots --json --limit 20

# 全 CheckRecord のサマリ
heal checks --json | jq '.[].check_id'

# id でキャッシュレコードを取得（フル Findings 付き）
heal fix show <check_id> --json
```

## Calibration

`calibration.toml` は Severity を扱う各メトリクスのコードベース相
対パーセンタイル区切りを保持します。`heal init` が初回スキャンか
ら計算し、`heal calibrate --force` がオンデマンドで更新します。
post-commit ナッジは `Calibration::with_overrides(config)` 経由で
読むため、`config.toml` の `floor_critical` / `floor_ok` は calibrate
されたパーセンタイルに勝ちます。

再 calibrate は **絶対に自動では行いません**。デフォルトの
`heal calibrate` は自動検知トリガー（90 日経過、コードベースファ
イル数 ±20%、30 日連続で Critical が 0）を評価して推奨を表示する
だけです。実行するかは常にユーザーが判断し、`heal calibrate
--force` を実行します。

監査トレイルは `.heal/snapshots/` に `event = "calibrate"` として
残ります。`MetricsSnapshot::latest_in_segments` はスナップショット
としてデコードできないレコードを静かにスキップするため、2 種類のイ
ベント形が干渉なく同居します。

## Calibration と policy: 2 つのレイヤ

heal はコード健全性の **測定** と、それに対して何を行うかの **意図**
を分離しています。

- **Calibration レイヤ**（`.heal/calibration.toml` + metric ごとの
  `[metrics.<m>]` override）は「この Finding は赤か？」を判定。
  3 段の分類器 — `floor_critical`（逃げ道）+ `floor_ok`（卒業ゲー
  ト、proxy メトリクスのみ）+ パーセンタイル区切り — が Severity
  を生成します。このレイヤは測定の問い: 値が文献閾値とプロジェクト
  分布に対してどこに位置するか、に答えます。
- **Policy レイヤ**（`config.toml` の `[policy.drain]`）は「その
  Finding はアクション対象か？」を判定。`(Severity, hotspot)` の組
  が 3 つの drain tier (T0 / `must`、T1 / `should`、Advisory) のいず
  れかにマップされます。このレイヤは意図の問い: チームが何を drain
  するとコミットするか、に答えます。

両レイヤは直交しています — 再 calibrate は Severity 境界を動かしま
すが policy には触れません。逆に policy を厳しく/緩くしても観測は
再実行されません。チームは通常 calibration を文献デフォルト近くに保
ち、自分たちの帯域に合わせて `[policy.drain]` を調整します。

## Drain queue モデル

`heal check` は非 Ok の Finding を `[policy.drain]` 駆動で 3 つのバ
ケットに分けます。

| Tier | デフォルト spec | レンダラー挙動 | Skill 挙動 |
| --- | --- | --- | --- |
| **T0 / Drain queue** | `must = ["critical:hotspot"]` | 常に表示、Severity 🔥 desc 順。 | `/heal-code-patch` が 1 finding ずつ drain。 |
| **T1 / Should drain** | `should = ["critical", "high:hotspot"]` | デフォルト表示、別セクション。 | レビュー対象、自動 drain しない。 |
| **Advisory** | それ以外の非 Ok | `--all` 時のみ表示。 | 自動 drain なし、余裕のあるときに review。 |

`Severity::Ok` の Finding は drain 対象外です。レンダラーは Ok 🔥
pre-section（上位 10% hotspot だがメトリクスフロア未満）と隠し合計
カウントで表示します。

Override の可視化: `[metrics.<m>] floor_ok` / `floor_critical` が文
献デフォルトと異なる場合、`heal check` はヘッダ行に
`override: ccn floor_ok=15 [override from 11]` のような注釈を出力し
ます。CI ログや PR diff で policy 変更が監査可能になります。

`[policy.drain]` の DSL は `<severity>`（hotspot 不問）または
`<severity>:hotspot`（hotspot=true 必須）。Severity トークンは小文字:
`critical / high / medium / ok`。詳細は[設定 › Drain ポリシー](/heal/ja/configuration/#drain-ポリシー)
を参照。
