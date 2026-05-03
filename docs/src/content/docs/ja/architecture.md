---
title: アーキテクチャ
description: heal がどこにデータを置き、いつ何が書き出されるか、各要素がどう組み合わさるか。
---

このページでは、heal がどんなファイルを作り、いつ書き出され、何を含むかを説明します。ナッジが出ない原因を調べたり、JSON 出力に対してスクリプトを書いたり、heal がバックグラウンドで何をしているのかを理解したいときに役立ちます。

## 全体像

```
git commit
    │
    ▼
.git/hooks/post-commit  ──►  heal hook commit
                                  │
                                  ├──►  オブザーバー（LOC, complexity, churn, …, lcom）
                                  │       (run_all を 1 回; 結果は下流で使用)
                                  │
                                  └──►  stdout: Severity ナッジ
                                         (Critical / High Finding のみ)

ユーザー: heal status（または `claude /heal-code-patch`）
    │
    ▼
heal status  ──►  calibration.toml で Finding を分類
                       │
                       ├──►  .heal/findings/latest.json
                       │       (FindingsRecord — TODO リスト)
                       │
                       ├──►  fixed.json ↔ regressed.jsonl を整合
                       │
                       └──►  Severity ごとのビューを stdout に描画
```

`heal` は単一バイナリです。両方の経路がこれを通ります。デーモンも、スケジューラも、バックグラウンドプロセスも、履歴ストリームも一切ありません。post-commit フックは全オブザーバーを **一度だけ** 実行してナッジを出し、終了します — 永続化は行いません。

## オンディスクのレイアウト

`heal init` 直後:

```
<your-repo>/
├── .heal/
│   ├── .gitignore                # 自動 — findings/ を除外
│   ├── config.toml               # 自分で編集する（git 管理対象）
│   ├── calibration.toml          # 自動 — heal init / heal calibrate（git 管理対象）
│   └── findings/
│       ├── latest.json           # 現在の FindingsRecord（TODO リスト）
│       ├── fixed.json            # BTreeMap<finding_id, FixedFinding> — 有界
│       └── regressed.jsonl       # 再検出された修正の追記専用監査トレイル
│
├── .git/hooks/post-commit         # `heal hook commit` を呼ぶ 1 行のシム
│
└── .claude/skills/                # Claude スキル群（`heal skills install` 後）
    ├── heal-cli/
    ├── heal-code-patch/
    ├── heal-code-review/
    └── heal-config/
```

`config.toml` と `calibration.toml` は git で追跡され、チームで同じ Severity ラダーを共有できます。`findings/` は `.heal/.gitignore` によって除外されます。

## 何がいつ書かれるか

| ファイル / ディレクトリ          | 書き出し元                                          | タイミング                                        |
| -------------------------------- | --------------------------------------------------- | ------------------------------------------------- |
| `.heal/.gitignore`               | `heal init`                                         | セットアップ時に一度。                            |
| `.heal/config.toml`              | `heal init`                                         | セットアップ時に一度。自由に編集可。              |
| `.heal/calibration.toml`         | `heal init` / `heal calibrate`                      | セットアップ時、その後は明示的な再 calibrate 時。 |
| `.heal/findings/latest.json`     | `heal status`                                       | 新規 `heal status`（キャッシュミス経路）ごと。    |
| `.heal/findings/fixed.json`      | `heal mark-fixed`（`/heal-code-patch` から呼出）    | `/heal-code-patch` のコミット着地ごと。           |
| `.heal/findings/regressed.jsonl` | `heal status`（整合パス）                            | 修正済み Finding が再検出されたとき。            |
| `.claude/skills/heal-*/`         | `heal skills install`                               | 一度だけ。`heal skills update` で更新。           |

イベントログも、月次ローテーションも、`.heal/snapshots/` / `.heal/logs/` / `.heal/docs/` / `.heal/reports/` も存在しません。heal は現在の状態と `regressed.jsonl` の小さな監査トレイルだけを保持します。

## findings キャッシュ

`.heal/findings/` には 3 つの成果物が並びます。`latest.json` の writer は `heal status` だけ、`fixed.json` / `regressed.jsonl` の writer は `heal mark-fixed` だけです。

### `latest.json` — 現在の TODO

```json
{
  "version": 2,
  "id": "01HKM3Q6Z1B7…",                 // ULID
  "started_at": "2026-04-30T05:14:22Z",
  "head_sha": "a0a6d1a…",
  "worktree_clean": true,
  "config_hash": "9f8e7d6c5b4a3210",     // config + calibration の FNV-1a
  "severity_counts": { "critical": 2, "high": 5, "medium": 12, "ok": 84 },
  "findings": [ /* Vec<Finding> */ ]
}
```

`heal status` は `(head_sha, config_hash, worktree_clean=true)` がキャッシュレコードと一致するときショートサーキットします — 同じコミット上での再実行は無料です。

### `fixed.json` — 有界の修正記録

`BTreeMap<finding_id, FixedFinding>` を 1 つの JSON オブジェクトとしてシリアライズしたもの。各エントリは決定論的な `finding_id` をキーにします。

```json
{
  "ccn:src/payments/engine.ts:processOrder:9f8e…": {
    "commit_sha": "a1b2c3",
    "fixed_at": "2026-04-30T05:14:22Z"
  }
}
```

有界です — 追記専用ではありません。新規 `heal status` で過去に fixed だった `finding_id` が再出現すると、heal は `fixed.json` から取り除いて `regressed.jsonl` に 1 行追記し、レンダラーが警告を出します。

### `regressed.jsonl` — 監査トレイル

`.heal/` 配下で唯一の追記専用ファイルです。再検出イベントごとに JSON を 1 行追加し、「修正したはずが再検出された」という警告を表示するためだけに使います。

このキャッシュは `jq` で直接覗けます。

```sh
jq '.severity_counts' .heal/findings/latest.json
jq 'keys | length'    .heal/findings/fixed.json
tail .heal/findings/regressed.jsonl
```

## Calibration

`calibration.toml` は Severity を扱う各メトリクスのコードベース相対パーセンタイル区切りを保持します。`heal init` が初回スキャンから計算し、`heal calibrate --force` がオンデマンドで更新します。`config.toml` の `floor_critical` / `floor_ok` は calibrate されたパーセンタイルに勝ちます。再 calibrate は **絶対に自動では行いません** — [CLI › `heal calibrate`](/heal/ja/cli/#heal-calibrate) を参照。

## Calibration と policy: 2 つのレイヤ

heal はコード健全性の **測定** と、それに対して何を行うかの **意図** を分離しています。

- **Calibration レイヤ**（`.heal/calibration.toml` + metric ごとの `[metrics.<m>]` override）は「この Finding は赤か？」を判定。3 段の分類器 — `floor_critical`（逃げ道）+ `floor_ok`（卒業ゲート、proxy メトリクスのみ）+ パーセンタイル区切り — が Severity を生成します。このレイヤは測定の問い: 値が文献閾値とプロジェクト分布に対してどこに位置するか、に答えます。
- **Policy レイヤ**（`config.toml` の `[policy.drain]`）は「その Finding はアクション対象か？」を判定。`(Severity, hotspot)` の組が 3 つの drain tier (T0 / `must`、T1 / `should`、Advisory) のいずれかにマップされます。このレイヤは意図の問い: チームが何を drain するとコミットするか、に答えます。

両レイヤは直交しています — 再 calibrate は Severity 境界を動かしますが policy には触れません。逆に policy を厳しく/緩くしても観測は再実行されません。チームは通常 calibration を文献デフォルト近くに保ち、自分たちの帯域に合わせて `[policy.drain]` を調整します。

## Drain queue モデル

`heal status` は非 Ok の Finding を `[policy.drain]` 駆動で 3 つのバケットに分けます。

| Tier | デフォルト spec | レンダラー挙動 | Skill 挙動 |
| --- | --- | --- | --- |
| **T0 / Drain queue** | `must = ["critical:hotspot"]` | 常に表示、Severity 🔥 desc 順。 | `/heal-code-patch` が 1 finding ずつ drain。 |
| **T1 / Should drain** | `should = ["critical", "high:hotspot"]` | デフォルト表示、別セクション。 | レビュー対象、自動 drain しない。 |
| **Advisory** | それ以外の非 Ok | `--all` 時のみ表示。 | 自動 drain なし、余裕のあるときに review。 |

`Severity::Ok` の Finding は drain 対象外です。レンダラーは Ok 🔥 pre-section（上位 10% hotspot だがメトリクスフロア未満）と隠し合計カウントで表示します。

Override の可視化: `[metrics.<m>] floor_ok` / `floor_critical` が文献デフォルトと異なる場合、`heal status` はヘッダ行に `override: ccn floor_ok=15 [override from 11]` のような注釈を出力します。CI ログや PR diff で policy 変更が監査可能になります。

`[policy.drain]` の DSL は `<severity>`（hotspot 不問）または `<severity>:hotspot`（hotspot=true 必須）。Severity トークンは小文字: `critical / high / medium / ok`。詳細は[設定 › Drain ポリシー](/heal/ja/configuration/#drain-ポリシー)を参照。
