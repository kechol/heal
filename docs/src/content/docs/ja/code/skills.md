---
title: Code · スキル
description: 常時オンの Code ファミリ向け Claude Code スキル 4 種 — /heal-cli、/heal-config、/heal-code-review、/heal-code-patch。
---

heal は Claude Code 向けスキルセットを同梱しているので、収集したメトリクスは Claude セッションへ自動的に流れます。リポジトリごとに 1 回だけインストールします:

```sh
heal skills install
```

このページは 4 つの Code ファミリスキルを扱います。doc 系は [Docs › スキル](/heal/ja/docs/skills/)、test 系は [Test › スキル](/heal/ja/test/skills/) を参照。

```
.claude/skills/
├── heal-cli/
├── heal-code-patch/
├── heal-code-review/
└── heal-config/
```

スキルセットは `heal` バイナリに同梱されているので、インストールされるバージョンは常にバイナリと一致します。`heal` をアップグレードしたら `heal skills update` で更新します。

## `/heal-code-review` — 監査スキル

読み取り専用です。`heal status --all --json` を取り込み、フラグ付きコードを深く読み、次の 2 つを返します:

1. コードベースの **アーキテクチャ的読解** — findings を _システムとして_ 何を語っているか(支配的な軸: complexity、duplication、coupling、hub)。
2. **優先順位付き TODO リスト** — デフォルトでは **T0(`must`)のみ**。T1(`should`)は別の「帯域があれば」セクションに、Advisory はカウントだけ表示します。

`/heal-code-review` は提案のみで、ソースは編集しません。チームが本質的(意図的に複雑な税金エンジン、手続き的に凝集したパーサコンビネータなど)と判断した findings には `heal mark accept` も推奨できます。

書き込み側のカウンターパートは `/heal-code-patch` です。

トリガーフレーズ: 「review the codebase health」、「what does heal say?」、「where should we refactor?」、「/heal-code-review」。

## `/heal-code-patch` — 書き込みスキル

`.heal/findings/latest.json` を Severity 順に 1 件ずつ drain し、修正ごとに 1 コミットします。

事前チェック(失敗すると起動拒否):

1. **クリーンな worktree。** dirty な worktree はキャッシュの `worktree_clean = false` を意味し、記録された数値はディスク上のソースを反映していません。スキルは止まり、commit か stash を依頼します。
2. **キャッシュ存在。** `latest.json` が無ければ `heal status --json` を 1 回走らせて埋めます。
3. **キャリブレーション存在。** `calibration.toml` が無ければすべての Finding が `Severity::Ok` で、アクション可能なものはありません。

ループは **T0(`must`)のみ** drain します。T1 / Advisory はレビュー用に表示しますが自動 drain はしません。T0 が空になったらセッションを終えます。

メトリクス別に、確立されたリファクタ語彙(Fowler、Tornhill)へマッピングされます:

| メトリクス                  | 主な手 |
| --------------------------- | -------------------------------------------------------------------------------------- |
| `ccn` / `cognitive`         | Extract Function、Replace Nested Conditional with Guard Clauses、Decompose Conditional |
| `duplication`               | Extract Function / Method、Pull Up Method、Form Template Method、Rule of Three         |
| `change_coupling`           | アーキテクチャの継ぎ目を可視化(coupling の自動修正は行わない)                       |
| `change_coupling.symmetric` | 同上(強い「責務混在」シグナルは人間判断が必要)                                       |
| `lcom`                      | クラスタ境界に沿ってクラスを分割(通常は Extract Class)                               |
| `hotspot`                   | Hotspot はフラグであって問題そのものではない。基底にある CCN / dup / coupling に対処 |

スキルが強制する制約:

- 1 finding = 1 commit。findings 横断の squash はしません。
- Conventional Commit の subject + body + `Refs: F#<finding_id>` トレーラ。
- push しない、amend しない、`--no-verify` しない。
- ループはキャッシュの境界で止まります。新たな findings は次の `heal status` 実行で取り込みます。

`/heal-code-patch` は doc / test ファミリのメトリクスに属する findings をスキップします。それぞれ `/heal-doc-patch` と `/heal-test-patch` の担当です。

トリガーフレーズ: 「fix the heal findings」、「drain the cache」、「work through the TODO list heal produced」、「/heal-code-patch」。

## `/heal-cli` — CLI リファレンス

`heal` CLI の簡潔で完全なリファレンスです。各サブコマンド、各 `--json` の形、各コマンドが読み書きする `.heal/` ファイルを網羅しています。Claude が他のスキルから `heal` をシェル実行する前にこれを読み込むので、CLI 表面は `--help` テキストから推論する対象ではなく、安定した契約として扱われます。

## `/heal-config` — キャリブレーションと config 調整

プロジェクトをキャリブレーションし、コードベースを見渡し、ユーザに strictness レベル(Strict / Default / Lenient)を選んでもらい、それに応じて `.heal/config.toml` を書く / 更新します。

使うタイミング:

- heal を初めてセットアップするとき。
- コードベースの構造変更(新しい vendored ツリー、レイヤ書き直しなど)の後。
- すべての閾値を覚えていなくても品質バーを動かしたいとき。

`/heal-config` はキャリブレーションのベースラインがドリフトして無視できなくなったときに `heal calibrate --force` も推奨します(ファイル数が大きく動いた、プロジェクトの速度に対してキャリブレーションが古い、Critical を持続的に drain し終えた、など)。

`[features.docs]` や `[features.test]` を有効にしている場合は、対応する `[calibration.*]` 表も拾い、ファミリ固有の調整値のオプトインデフォルトを提案します。

## 更新

```sh
heal skills update
```

`update` はドリフト認識付きで、手編集されたファイルはそのまま残します(警告付き)。`--force` ですべて上書きできます。`heal skills status` でドリフト状況を確認できます。

## アンインストール

```sh
heal skills uninstall
```

`.claude/skills/heal-*` 配下のすべての同梱スキルディレクトリを削除します(展開されていれば doc / test ファミリも含む)。あなたが書いた兄弟スキルは残り、`.heal/` 配下のプロジェクトデータも基本的に手付かずです。
