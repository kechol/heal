---
title: アーキテクチャ
description: heal がどこにデータを置き、いつ何を書き出し、各部分がどう組み合わさっているか。
---

このページでは、heal がどんなファイルを作り、いつ書き出し、それぞれに何が入っているかを説明します。コミット時の通知が出ないときの調査や、JSON 出力を使ったスクリプトづくりに役立ちます。heal が裏で何をしているのかを知りたいときにも読んでください。

## 全体像

```
git commit
    │
    ▼
.git/hooks/post-commit  ──►  heal hook commit
                                  │
                                  ├──►  オブザーバ（LOC, complexity, churn, …, lcom）
                                  │       （run_all を 1 回実行し、結果をその下で使う）
                                  │
                                  └──►  標準出力: Severity の通知
                                         （Critical / High の Finding だけ）

利用者: heal status（または Claude Code で `/heal:refactor`）
    │
    ▼
heal status  ──►  calibration.toml で Finding を分類
                       │
                       ├──►  .heal/findings/latest.json
                       │       （FindingsRecord — TODO リスト）
                       │
                       ├──►  fixed.json と regressed.jsonl を突き合わせる
                       │
                       └──►  Tier / Severity / スコアの順に並べて標準出力に表示
```

`heal` は 1 つのバイナリで、どちらの経路もこのバイナリを通ります。デーモンも、スケジューラも、バックグラウンドプロセスも、履歴を書き足していく仕組みもありません。post-commit フックはすべてのオブザーバを **1 回だけ** 実行し、通知を表示して終わります。何も保存しません。

## オンディスクのレイアウト

`heal init` の直後は次のようになります。

```
<your-repo>/
├── .heal/
│   ├── config.toml                # 自分で編集する（git で管理）
│   ├── calibration.toml           # 自動生成 — heal init / heal calibrate（git で管理）
│   └── findings/                  # git で管理 — チームで TODO を共有する
│       ├── latest.json            # 今の FindingsRecord（TODO リスト）
│       ├── fixed.json             # 修正の記録（件数に上限がある）
│       ├── accepted.json          # 「直さない・本質的」と判断したものの置き場
│       └── regressed.jsonl        # 直したはずの Finding が再び見つかった記録（追記のみ）
│
└── .git/hooks/post-commit         # 1 行だけのスクリプト: `heal hook commit` を呼ぶ
```

Claude のスキルはリポジトリには置きません。heal の Claude Code プラグインから入り、Claude Code が自分の設定ディレクトリで管理するので、入れても更新しても git 管理下のファイルは変わりません。heal 0.6 以前はスキルを `.claude/skills/heal-*` と `.agents/skills/heal-*` にコピーしていました。そのコピーは `heal skills uninstall` で消せます。

`config.toml`、`calibration.toml`、`findings/` ディレクトリは、どれも git で管理します。同じコミットにいるチームメンバーは、同じ Severity の段階と、同じ解消キューを共有します。

## 何がいつ書かれるか

| ファイル / ディレクトリ          | 書き出すもの                                                                  | タイミング                                                |
| -------------------------------- | ----------------------------------------------------------------------------- | --------------------------------------------------------- |
| `.heal/config.toml`              | `heal init`                                                                   | セットアップ時に 1 度。あとは自由に編集できる。           |
| `.heal/calibration.toml`         | `heal init` / `heal calibrate`                                                | セットアップ時と、明示的に calibration をやり直したとき。 |
| `.heal/findings/latest.json`     | `heal status`                                                                 | キャッシュが使えず、`heal status` が走査し直したとき。    |
| `.heal/findings/fixed.json`      | `heal mark fix`（`/heal:refactor`、`/heal:docs`、`/heal:tests` から呼ばれる） | それらのスキルがコミットするたび。                        |
| `.heal/findings/accepted.json`   | `heal mark accept`（同じスキルが、承認を得てから呼ぶ）                        | チームが本質的な Finding を受け入れたとき。               |
| `.heal/findings/regressed.jsonl` | `heal status`（突き合わせの処理）                                             | 直したはずの Finding が再び見つかったとき。               |
| `.heal/doc_pairs.json`           | `/heal:setup` スキル（`[features.docs]` が有効なとき）                        | 利用者がスキルを実行したとき。HEAL は読むだけ。           |
| `.heal/concepts.toml`            | `/heal:setup` スキル（`[features.semantic]` が有効なとき）                    | 利用者がスキルを実行したとき、または語彙を編集したとき。  |
| `.heal/semantic/verdicts/`       | `heal semantic ask`                                                           | 新しく問い合わせるたび。コードと一緒にコミットする。      |
| `.heal/cache/source-v1.json`     | ソースのオブザーバ                                                            | ソースの解析結果が変わったとき。消しても問題ない。        |
| `.heal/cache/semantic/verdicts/` | `heal semantic ask`（その場で行う確認、`focus`）                              | そうした確認を実行するたび。消しても問題ない。            |

イベントログも、月ごとのローテーションも、`.heal/snapshots/`、`.heal/logs/`、`.heal/reports/` といったディレクトリもありません。heal が持つのは、今の状態と、`regressed.jsonl` の小さな監査記録だけです。

`.heal/cache/` は、git で管理する findings のキャッシュとは別物です。中身は、あとから作り直せるデータだけです。入っているのは、変更のないソースファイルの Complexity・LCOM・Duplication のトークンデータと、一人ひとりの作業についての semantic の確認結果です。ディレクトリ自身が、git の管理から外れるように設定されています。heal は使い回す前に毎回ファイルの中身を検証します。ディレクトリを消しても、中身が壊れていても、書き込みに失敗しても、通常どおりソースを解析するだけです。

例外は `.heal/docs/` です。`/heal:docs scaffold` を実行すると、生成したドキュメントのツリーを `[features.docs] scaffold_root` で指定したディレクトリ（既定は `.heal/docs/`）に書き出します。HEAL 自身はこのツリーを**読む**だけで、書くのはスキルだけです。

## Findings キャッシュ

`.heal/findings/` には 4 つのファイルがあります。`latest.json` と `regressed.jsonl` を書くのは `heal status` だけ、`fixed.json` を書くのは `heal mark fix` だけ、`accepted.json` を書くのは `heal mark accept` だけです。

### `latest.json` — 今の TODO

```json
{
  "version": 9,
  "id": "9f8e7d6c5b4a3210", // (head_sha, config_hash, worktree_clean) の FNV-1a（16 進）
  "head_sha": "a0a6d1a…",
  "worktree_clean": true,
  "config_hash": "9f8e7d6c5b4a3210", // ルールと、有効にしている git 以外の入力
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

`heal status` は、`(head_sha, config_hash, worktree_clean=true)` がキャッシュの記録と一致すれば、走査を省きます。`config_hash` には、`config.toml`、`calibration.toml`、そして有効にしている LCOV とドキュメントのペアのファイルそれぞれについての論理パス、読めるかどうか、中身が入ります。ファイルの更新時刻、チェックアウトした場所の絶対パス、無効にしたファミリの入力は入りません。そのため、git の管理外にある LCOV レポートやペアのファイルが変わると、HEAD が同じでも結果は使い回しません。一方で、観測した内容が同じなら、どこにチェックアウトしてもバイト単位で同じ結果になります。履歴を見る期間は、実行時の時計ではなく、観測した HEAD（または指定した参照）のコミット時刻を基準にします。

スキーマ v6 でこの広い範囲のハッシュが入り、v7 でカバレッジの観測状態、v8 で任意の `findings[].hotspot_score` が加わりました。古いバージョンのキャッシュは捨てられ、自動で作り直されます。LCOV に載っていない本番ファイルは「未計測」として報告し、0% とみなして Finding を作ることはしません。LCOV に 0% と明記された記録は、計測済みの Finding として扱います。

### `fixed.json` — 修正の記録

`BTreeMap<finding_id, FixedFinding>` を 1 つの JSON オブジェクトにしたものです。各エントリのキーは、決まった手順で作る `finding_id` です。

```json
{
  "ccn:src/payments/engine.ts:processOrder:9f8e…": {
    "commit_sha": "a1b2c3",
    "fixed_at": "2026-04-30T05:14:22Z"
  }
}
```

件数に上限があり、追記するだけのファイルではありません。一度直した `finding_id` が新しい `heal status` で再び現れると、heal はそれを `fixed.json` から外し、`regressed.jsonl` に 1 行書きます。表示でも警告が出ます。

### `regressed.jsonl` — 監査記録

`.heal/` の中で、追記だけを行う唯一のファイルです。直したはずの Finding が戻ってくるたびに JSON を 1 行書きます。使い道は「修正が再び見つかった」という警告を出すことだけです。

### `accepted.json` — 「直さない・本質的」の置き場

`BTreeMap<finding_id, AcceptedFinding>` を 1 つの JSON オブジェクトにしたものです。書き出すのは `heal mark accept` で、`/heal:refactor`、`/heal:docs`、`/heal:tests` から呼ばれます。チームが「この Finding は本質的なものなので解消しない」と記録することを承認したときです。

```json
{
  "ccn:src/payments/engine.ts:processOrder:9f8e…": {
    "reason": "intrinsic — branchy by design (tax engine)",
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

`fixed.json` とは違い、Finding が再び現れても受け入れのエントリは消えません。受け入れた Finding は期限なく解消キューから外れます。`heal status` では、ヘッダーに `Accepted: N findings` の 1 行が出て、`--all` を付けると `📌 Accepted` のセクションに表示されます。

`Finding.accepted: bool` は、表示するときに `accepted.json` を Finding の一覧に重ねて付けます。`latest.json` そのものにはオブザーバの生の結果だけを残し、`accepted: true` は書き込みません。そのため、受け入れを付けたり外したりしても走査し直す必要はありません。

エントリを消すには、ファイルを手で編集してその行を削除するか、スキルの流れの中で `heal mark accept --remove` を呼びます。次の `heal status` で、元の Finding がまた表示されます。

受け入れ済みの Finding でも、Severity が上がったときや、そのファミリの Hotspot が false から true に変わったときは、見直しを求めます。求めるのは status、diff の現在側、post-commit フックです。JSON には、理由（`severity_increased`、`became_hotspot` の一方または両方）を持つ `accepted_rereview` オブジェクトが 1 つ入ります。受け入れは取り消されず、Finding が T0 に戻ることもありません。変化がない、落ち着いてきた、良くなったという受け入れ済みの Finding については何も知らせません。

キャッシュは `jq` で直接見られます。

```sh
jq '.severity_counts' .heal/findings/latest.json
jq 'keys | length'    .heal/findings/fixed.json
jq 'keys | length'    .heal/findings/accepted.json
tail .heal/findings/regressed.jsonl
```

## Calibration

`calibration.toml` には、Severity を持つすべてのメトリクスについて、コードベースを基準にしたパーセンタイルの区切りが入っています。`heal init` が最初の走査から計算し、`heal calibrate --force` で必要なときに作り直します。`config.toml` で設定した `floor_critical` / `floor_ok` は、calibration で求めたパーセンタイルより優先されます。calibration のやり直しが**自動で行われることはありません**。[CLI › `heal calibrate`](/heal/ja/cli/#heal-calibrate) を参照してください。

Hotspot は、有限の候補が 5 件以上あれば p90 とファミリのフロアを使います。1〜4 件のときは、ファミリごとに決まっている絶対値のフロア（Code 22、Test 25、Docs 5）だけを使い、有限でない値はフラグの対象にしません。この代わりの判定は、パーセンタイルでも、確率の見積もりでも、修正の効果の保証でもありません。

## Calibration と policy: 2 つのレイヤ

heal は、コードの健全性を*測ること*と、何に手を付けるかという*意図*を分けています。

- **calibration のレイヤ**（`.heal/calibration.toml` と、メトリクスごとの `[metrics.<m>]` の上書き）は、「この Finding は赤か」を決めます。`floor_critical`（逃げ道）、`floor_ok`（卒業の基準。代理指標のメトリクスだけ）、パーセンタイルの区切りの 3 つを組み合わせて Severity を出します。答えるのは測定の問いです。この値は、文献のしきい値と、プロジェクト自身の分布に対してどこにあるか。
- **policy のレイヤ**（`config.toml` の `[policy.drain]`）は、「この Finding に手を付けるべきか」を決めます。`(Severity, hotspot)` の組を、T0 / `must`、T1 / `should`、Advisory の 3 つの解消 Tier のどれかに割り当てます。答えるのは意図の問いです。チームは何を解消すると約束するか。

2 つのレイヤは独立しています。calibration をやり直しても Severity の境目が動くだけで、policy には触れません。policy を厳しくしても緩くしても、解消の扱いが変わるだけで、オブザーバを実行し直す必要はありません。多くのチームは、calibration を文献の既定値に近いまま保ち、自分たちの手の空き具合に合わせて `[policy.drain]` を調整しています。

## 解消キューのモデル

`heal status` は、Ok 以外のすべての Finding を、`[policy.drain]` に従って 3 つのグループのどれかに分けます。

| Tier                  | 既定の指定                              | 表示                               | スキルの動き                                           |
| --------------------- | --------------------------------------- | ---------------------------------- | ------------------------------------------------------ |
| **T0 / Drain queue**  | `must = ["critical:hotspot"]`           | 常に表示。決まった優先順で並ぶ。   | 最初に提案する。承認されたら 1 提案 1 コミットで適用。 |
| **T1 / Should drain** | `should = ["critical", "high:hotspot"]` | 既定で表示。別のセクションになる。 | T0 の後に提案する。                                    |
| **Advisory**          | それ以外の、Ok より上のもの             | `--all` を付けたときだけ表示。     | 文脈として使うだけ。単独では提案しない。               |

`Severity::Ok` に分類された Finding は、解消の対象から完全に外れます。`--all` を付けると、通常の Ok のセクションにスコア順で表示されます。Hotspot とそうでないものが混ざったセクションでは、Hotspot の行に `🔥` が付きます。`--all` を付けなければ、隠れた項目の件数に数えられるだけです。

各ファミリの各 Tier の中では、行は Severity、`hotspot_score` の降順の順に並び、同点ならメトリクス、パス、id で安定して決めます。Code・Test・Docs のスコアを互いに比べることはありません。

上書きの見える化: `[metrics.<m>] floor_ok` や `floor_critical` が文献の既定値と違うと、`heal status` はヘッダーに `override: ccn floor_ok=15 [override from 11]` のような行を出します。policy の変更が、CI のログや PR の差分から追えるようにするためです。

`[policy.drain]` の書き方は、`<severity>`（Hotspot かどうかを問わない）か `<severity>:hotspot`（hotspot=true が必要）です。Severity は小文字で `critical / high / medium / ok` と書きます。[Code › 設定 › `[policy.drain]`](/heal/ja/code/configuration/#policydrain) を参照してください。
