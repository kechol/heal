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

`heal` は単一バイナリです。両方の経路がこれを通ります。デーモンも、スケジューラも、バックグラウンドプロセスも、履歴ストリームも一切ありません。post-commit フックは全オブザーバを **一度だけ** 実行してナッジを出し、終了します(永続化は行いません)。

## オンディスクのレイアウト

`heal init` 直後:

```
<your-repo>/
├── .heal/
│   ├── .gitignore                # 自動 — 将来用の予約(現状は空)
│   ├── config.toml               # 自分で編集する(git 追跡対象)
│   ├── calibration.toml          # 自動 — heal init / heal calibrate(git 追跡対象)
│   └── findings/                 # git 追跡対象 — チームで TODO を共有
│       ├── latest.json           # 現在の FindingsRecord(TODO リスト)
│       ├── fixed.json            # 修正済み記録の有界マップ
│       ├── accepted.json         # 「直さない」と判断した finding の記録
│       └── regressed.jsonl       # 再検出された修正の追記専用監査トレイル
│
├── .git/hooks/post-commit         # `heal hook commit` を呼ぶ 1 行のシム
│
└── <agent skills>                 # 検出した各エージェントごとに 1 ツリー(`heal init --yes` 後)
    │                              #   .claude/skills/   (Claude Code)
    │                              #   .agents/skills/   (OpenAI Codex)
    ├── heal-cli/                  # Code ファミリ
    ├── heal-code-patch/
    ├── heal-code-review/
    ├── heal-setup/
    ├── heal-doc-pair-setup/       # Docs ファミリ
    ├── heal-doc-scaffold/
    ├── heal-doc-review/
    ├── heal-doc-patch/
    ├── heal-test-reporter-setup/  # Test ファミリ
    ├── heal-test-review/
    └── heal-test-patch/
```

同梱の 11 スキルは検出した各エージェントへ同一バイト列で展開されます。`[features.docs]` や `[features.test]` を後から有効化したときに、すでにインストール済みのスキル本体がそのまま意味を持つようになる仕組みです(再展開は不要)。

`config.toml`、`calibration.toml`、`findings/` の中身はすべて git で追跡されるので、同じコミット上のチームメイトは同じ Severity ラダーと解消キューを共有できます。

## 何がいつ書かれるか

| ファイル / ディレクトリ          | 書き出し元                                                              | タイミング                                                         |
| -------------------------------- | ----------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `.heal/config.toml`              | `heal init`                                                             | セットアップ時に一度。自由に編集可。                               |
| `.heal/calibration.toml`         | `heal init` / `heal calibrate`                                          | セットアップ時、その後は明示的な再 calibrate 時。                  |
| `.heal/findings/latest.json`     | `heal status`                                                           | 新規 `heal status`（キャッシュミス経路）ごと。                     |
| `.heal/findings/fixed.json`      | `heal mark fix`（`/heal-code-patch` から呼出）                          | `/heal-code-patch` のコミット着地ごと。                            |
| `.heal/findings/accepted.json`   | `heal mark accept`（`/heal-code-review` から呼出）                      | チームが「設計上のもので直さない」と判断した項目を記録時。         |
| `.heal/findings/regressed.jsonl` | `heal status`（整合パス）                                               | 修正済み Finding が再検出されたとき。                              |
| `.heal/doc_pairs.json`           | `/heal-doc-pair-setup` スキル（`[features.docs]` 有効時）               | ユーザがスキルを実行したとき。HEAL は読み取り専用。                |
| `.heal/cache/source-v1.json`     | source observer                                                         | source解析結果が変わったとき。削除しても問題ありません。           |
| `<agent>/skills/heal-*/`         | `heal init`(検出した各エージェント)/ `heal skills install`(Claude のみ) | エージェントごとに一度。`heal init --force --yes` でリフレッシュ。 |

イベントログも、月次ローテーションも、`.heal/snapshots/` / `.heal/logs/` / `.heal/reports/` も存在しません。heal は現在の状態と `regressed.jsonl` の小さな監査トレイルだけを保持します。

`.heal/cache/` はgit追跡対象のFindings cacheとは別物です。変更のないsourceファイルについて、Complexity、LCOM、Duplication tokenの再生成可能なデータだけを保持し、自身をgitの対象外にします。再利用前には必ずファイル内容を検証します。ディレクトリを削除した場合や、cacheが壊れている、書き込めない場合は通常のsource解析へ戻ります。

`.heal/docs/` だけは例外で、`/heal-doc-scaffold` を実行すると `[features.docs] scaffold_root`(デフォルトは `.heal/docs/`)に生成済みドキュメントツリーが書き出されます。HEAL 自身はこのツリーを **読む** だけで、書くのはスキル側だけです。

## Findings キャッシュ(項目一覧の保管場所)

`.heal/findings/` には 4 つの成果物が並びます。`latest.json` と `regressed.jsonl` の writer は `heal status` だけ、`fixed.json` の writer は `heal mark fix` だけ、`accepted.json` の writer は `heal mark accept` だけです。

### `latest.json` — 現在の TODO

```json
{
  "version": 8,
  "id": "9f8e7d6c5b4a3210", // (head_sha, config_hash, worktree_clean) の FNV-1a hex
  "head_sha": "a0a6d1a…",
  "worktree_clean": true,
  "config_hash": "9f8e7d6c5b4a3210", // ルール + 有効な非 git 観測入力
  "severity_counts": { "critical": 2, "high": 5, "medium": 12, "ok": 84 },
  "coverage_observation": {
    "state": "partial",
    "configured_sources": ["lcov.info"],
    "sources": ["lcov.info"],
    "unreadable_sources": [],
    "unmeasured_files": ["src/new.ts"]
  },
  "findings": [/* Vec<Finding> */]
}
```

`heal status` は `(head_sha, config_hash, worktree_clean=true)` がキャッシュレコードと一致するときショートサーキットします。`config_hash` には `config.toml`、`calibration.toml`、有効な LCOV / doc-pair の論理パス・読取可否/欠落状態・内容が入ります。mtime、checkout の絶対パス、無効なファミリの入力は含みません。そのため、同じ HEAD でも ignored な LCOV や doc-pair が変われば再利用せず、同じ観測は checkout 間でバイト再現されます。履歴窓の基準は実行時刻ではなく観測対象 HEAD/ref のコミット時刻です。

スキーマ v6 でこの観測 hash、v7 で coverage provenance、v8 で optional な `findings[].hotspot_score` が入りました。古い cache は自動的に無効化され、再構築されます。LCOV にない production ファイルは 0% と合成せず未計測とし、LCOV が明示する 0% は実測 Finding のままです。

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

### `accepted.json` — 「直さない」レーン

`BTreeMap<finding_id, AcceptedFinding>` を 1 つの JSON オブジェクトとしてシリアライズしたもの。`heal mark accept` が writer で、`/heal-code-review` スキルが「この項目は設計上避けられないので解消対象から外す」という判断を記録するときに呼びます。

```json
{
  "ccn:src/payments/engine.ts:processOrder:9f8e…": {
    "reason": "設計上分岐が多い(税金エンジン)",
    "file": "src/payments/engine.ts",
    "metric": "ccn",
    "severity": "critical",
    "hotspot": true,
    "metric_value": 31.0,
    "summary": "CCN=31 processOrder (TypeScript)",
    "accepted_at": "2026-04-30T05:14:22Z",
    "accepted_by": "Alice <alice@example.com>"
  }
}
```

`fixed.json` とは異なり、accepted エントリは finding が再出現しても消費されません。解消キューでの存在を無期限に抑制し、`heal status` は `Accepted: N findings` ヘッダ行と、`--all` 指定時の `📌 Accepted` セクションでそれらを別途表示します。

`Finding.accepted: bool` はレンダリング時に `accepted.json` を finding リストに畳み込むことで装飾されます — `latest.json` 自体は raw observer truth を保ち、`accepted: true` を持ちません。これにより accept のオン / オフを切り替えても再スキャンは不要です。

エントリの削除: ファイルを手編集して該当行を消すか、スキルフローから `heal mark accept --remove` を呼びます。次の `heal status` で元の finding が再び表面化します。

accepted finding の Severity が上昇するか、そのファミリの Hotspot が false から true になると、status、現在側 diff、post-commit hook が再レビューを通知します。JSON は 1 つの `accepted_rereview` オブジェクトに 1 つまたは両方の理由 (`severity_increased`, `became_hotspot`) を返します。accept は解除せず T0 に戻しません。安定・冷却・改善時は通知しません。

このキャッシュは `jq` で直接覗けます。

```sh
jq '.severity_counts' .heal/findings/latest.json
jq 'keys | length'    .heal/findings/fixed.json
jq 'keys | length'    .heal/findings/accepted.json
tail .heal/findings/regressed.jsonl
```

## Calibration(Severity 基準の調整)

`calibration.toml` は Severity を扱う各メトリクスのコードベース相対パーセンタイル区切りを保持します。`heal init` が初回スキャンから計算し、`heal calibrate --force` がオンデマンドで更新します。`config.toml` の `floor_critical` / `floor_ok` は calibrate されたパーセンタイルに勝ちます。再 calibrate は **絶対に自動では行いません** — [CLI › `heal calibrate`](/heal/ja/cli/#heal-calibrate) を参照。

Hotspot は有限スコアが 5 件以上なら p90 + ファミリフロア、1〜4 件なら既存の絶対ファミリフロアのみ (Code 22、Test 25、Docs 5) で判定します。非有限値はフラグしません。この fallback はパーセンタイル、確率、修正効果の保証ではありません。

## Calibration(Severity 基準の調整)と policy: 2 つのレイヤ

heal はコード健全性の **測定** と、それに対して何を行うかの **意図** を分離しています。

- **Calibration レイヤ**（`.heal/calibration.toml` + metric ごとの `[metrics.<m>]` override）は「この Finding は赤か？」を判定。3 段の分類器 — `floor_critical`（逃げ道）+ `floor_ok`（卒業ゲート、proxy メトリクスのみ）+ パーセンタイル区切り — が Severity を生成します。このレイヤは測定の問い: 値が文献閾値とプロジェクト分布に対してどこに位置するか、に答えます。
- **Policy レイヤ**（`config.toml` の `[policy.drain]`）は「その Finding はアクション対象か？」を判定。`(Severity, hotspot)` の組が 3 つの drain tier (T0 / `must`、T1 / `should`、Advisory) のいずれかにマップされます。このレイヤは意図の問い: チームが何を drain するとコミットするか、に答えます。

両レイヤは直交しています — 再 calibrate は Severity 境界を動かしますが policy には触れません。逆に policy を厳しく/緩くしても観測は再実行されません。チームは通常 calibration を文献デフォルト近くに保ち、自分たちの帯域に合わせて `[policy.drain]` を調整します。

## 解消キュー モデル

`heal status` は非 Ok の Finding を `[policy.drain]` 駆動で 3 つのバケットに分けます。

| Tier                      | デフォルト spec                         | レンダラー挙動                 | Skill 挙動                               |
| ------------------------- | --------------------------------------- | ------------------------------ | ---------------------------------------- |
| **T0 / 解消キュー**       | `must = ["critical:hotspot"]`           | 常に決定的な優先順で表示。     | `/heal-code-patch` が 1 件ずつ解消。     |
| **T1 / 余裕があれば解消** | `should = ["critical", "high:hotspot"]` | デフォルト表示、別セクション。 | レビュー対象、自動解消 しない。          |
| **Advisory**              | それ以外の非 Ok                         | `--all` 時のみ表示。           | 自動解消 なし、余裕のあるときに review。 |

`Severity::Ok` の Finding は解消対象外です。`--all` では通常の Ok セクションにスコア順で表示し、hotspot/plain が混在する場合は該当行へ `🔥` を付けます。`--all` なしでは隠し合計カウントにだけ含めます。

各ファミリ・Tier の中では Severity、`hotspot_score` 降順、安定した metric/path/id の順で並びます。Code、Test、Docs の生スコアを相互に比較することはありません。

Override の可視化: `[metrics.<m>] floor_ok` / `floor_critical` が文献デフォルトと異なる場合、`heal status` はヘッダ行に `override: ccn floor_ok=15 [override from 11]` のような注釈を出力します。CI ログや PR diff で policy 変更が監査可能になります。

`[policy.drain]` の DSL は `<severity>`（hotspot 不問）または `<severity>:hotspot`（hotspot=true 必須）。Severity トークンは小文字: `critical / high / medium / ok`。詳細は[設定 › Drain ポリシー](/heal/ja/configuration/#drain-ポリシー)を参照。
