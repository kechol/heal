---
title: 設計思想
description: heal がなぜ必要なのか、どんな課題を解こうとしているのか、そしてコードベースに対してどうアプローチするのか。
---

このページでは、heal が何を考えて作られているのかを説明します。す
ぐに使い始めたい方は[クイックスタート](/heal/ja/quick-start/)から
どうぞ。

## 課題

Claude Code をはじめとする AI コーディングエージェントは、目の前の
変更を作るのが得意です。一方でコードベースのほうは、コミットを重ね
るたびに少しずつ姿を変えていきます。修正や機能追加を繰り返すうちに
複雑度が積み上がり、特定のファイルばかりが何度も書き換えられ、似た
ようなコードがじわじわ増えていく。こうした長期的な変化はエージェン
トの視野には入っておらず、外からのシグナルがなければ追いかけている
のは人間だけ、という状態になります。

小規模なプロジェクトであれば、それでも問題ありません。しかし、ある
程度の規模になると、「このファイル、最近触りにくくなったな」と気づ
いた頃にはもうリグレッションが本番に届いている、という後手の保守に
なりがちです。

## heal のアイデア

> **コードベースの健全性シグナルを、エージェントへのトリガーに変える。**

CI がテストの実行を監視するのと同じ感覚で、heal はコードベースの状
態を監視します。

- コミットのたびに、コードベースを計測する（複雑度、Churn、
  Duplication、Hotspot、LCOM）。
- すべての Critical / High Finding を、そのコミット出力の中で
  stdout に出す。
- オンデマンド（`heal status`）で、Finding を Severity 別に分類し、
  同梱の `/heal-code-patch` スキルが消化する TODO リストキャッシュを書き
  出す — 1 コミット 1 Finding ずつ。

その結果、「リンターを走らせるのを忘れないようにしないと」と人間が
気を張らなくても、エージェントの側に構造化された優先順位付き TODO
が届くようになります。post-commit フックは、デーモンやポーリングな
しで「次の一手」を見える状態に保ちます。

## ループ

heal は **observe → calibrate → check → fix** の 4 ステップで構成
されています。

```
コミット時
─────────────────────────────────────────────────

git commit
    │
    ▼
post-commit フック ──► heal hook commit
                          │
                          ├─ オブザーバーを 1 回走らせる
                          │
                          └─ Critical / High を stdout に出す

                    （ナッジを出すだけ、永続化はしない）


オンデマンド
─────────────────────────────────────────────────

heal status
    │
    ├─ .heal/calibration.toml で Finding を分類
    ├─ FindingsRecord を書く ──► .heal/findings/latest.json
    ├─ fixed.json ↔ regressed.jsonl を整合
    └─ Severity ごとのビューを描画


claude /heal-code-patch
    │
    └─ .heal/findings/latest.json を 1 コミット 1 Finding ずつ消化
       (Severity 順; Critical 🔥 が先頭)
```

## コードベース相対の Severity

素朴なしきい値（「CCN ≥ 10 は high」）はプロジェクト間でうまく機能
しません。200 行のスクリプトと 200kloc のサービスは別世界で動いて
います。heal は各メトリクスを **コードベース自身の分布** に合わせ
て calibrate します。

- 初回スキャンの `p75 / p90 / p95` が、文献由来の絶対フロアの下に
  ある状態のパーセンタイル区切りになります。
- フロア超え（または `p95` 超え）: Critical。
- `≥ p90`: High。
- `≥ p75`: Medium。
- それ以外: Ok。

`Hotspot` は **直交** します — Severity ではなくフラグ（Hotspot ス
コアの上位 10%）です。Finding は `Critical 🔥`、`Critical`、
`High 🔥` などになり得ます — レンダラーは別バケットとして並べます。

`heal calibrate --force` で再スキャンしてファイルを上書きします。
`--force` なしでファイルが既にある場合は no-op です。ドリフト検
知（calibration の経過、コードベースのファイル数変化、Critical 連
続ゼロ期間）は `/heal-config` スキルが担当し、`calibration.toml`
のメタ情報と現在の findings キャッシュを突き合わせて
`heal calibrate --force` を推奨します。再 calibrate は **絶対に自
動で行いません** — ベースラインのリセット時期は常にユーザーが決め
ます。

## デフォルトはリードオンリー、書き込みはスキル経由

`heal` CLI そのものはソースファイルを変更しません。修復は同梱の
`/heal-code-patch` Claude スキル経由で行われ、次の制約を持ちます。

- dirty な worktree では起動しない、
- 1 修正につき 1 コミット、
- push しない、
- amend しない。

`heal mark-fixed` が状態を mutate する唯一の CLI サブコマンドで、
`fixed.json` に `FixedFinding` を記録します。`/heal-code-patch` が
コミット後に呼ぶことを想定しています。

## なぜメトリクスなのか

heal には 7 つのメトリクスが付属しています。

- **LOC** — プロジェクトの言語構成
- **Complexity (CCN + Cognitive)** — 読むのがしんどい関数
- **Churn** — よく書き換えられているファイル
- **Change Coupling** — 一緒に変更されがちなファイル。one-way の
  leader/follower カウントと、symmetric（「責務の混在」）サブセッ
  トの両方
- **Duplication** — コピペで増えてしまったコード
- **Hotspot** — Churn × Complexity、「コードを犯罪現場として見る」
  視点
- **LCOM** — メソッド間で状態を共有していない（機械的に分割可能な）
  クラス

どれも昔から研究されてきた定番のメトリクスばかりで、AI 専用に作っ
たものは一つもありません。heal の貢献はメトリクス自体ではなく — そ
れらは何十年も前から存在しています — それらを **calibrate された
トリガー** としてエージェントのループに使い、人間をポーリング役か
ら解放するところにあります。

各メトリクスの式は[メトリクス](/heal/ja/metrics/)を参照してくださ
い。

## なぜフックで動かすのか

エージェントはコードを書くのは上手ですが、自分のまわりの状態を逐一
見てくれるわけではありません。フックを使えば、コードベースのほうか
ら自分でシグナルを出してもらうことができます。

**git の post-commit フック**は、コミットが入った瞬間にオブザー
バーを 1 回走らせて Severity ナッジを stdout に出します。デーモン
も、スケジューラも、永続化される状態もありません。heal は Claude
Code のフックを一切登録しません — ループは同梱スキルだけで完結し
ます。

## heal ではないもの

- **リンターではありません。** リンターは行単位で指摘します。heal
  は「どのファイルに、どの順で目を向けるべきか」を伝えます。
- **コードレビュアーでもありません。** その役は Claude が担当しま
  す。heal はそのプロンプトと TODO リストを整えるだけです。
- **CI ゲートでもありません。** post-commit フックはコミットが入っ
  たあとに動きます。個別の PR をブロックするのではなく、コードベー
  スの長期的な動きを追うのが目的です。
- **テストの代わりにもなりません。** heal が見つけるのはあくまで構
  造的な複雑さで、コードの正しさは引き続きテストの仕事です。

## さらに読む

- [クイックスタート](/heal/ja/quick-start/) — 実際のリポジトリでイ
  ンストールから動作確認まで
- [メトリクス](/heal/ja/metrics/) — 各メトリクスの中身と Severity
  の付け方
- [CLI](/heal/ja/cli/) — 全コマンド（`heal status`、`heal diff`、
  `heal metrics`、`heal calibrate`）
- [設定](/heal/ja/configuration/) — `.heal/config.toml` と
  `.heal/calibration.toml` のリファレンス
- [アーキテクチャ](/heal/ja/architecture/) — オンディスクレイアウ
  ト、イベントストリーム、キャッシュ契約
- [Claude スキル](/heal/ja/claude-skills/) — `/heal-code-review`、
  `/heal-code-patch`、`/heal-cli`、`/heal-config`
