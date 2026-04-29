---
title: Claude プラグイン
description: 同梱の Claude Code プラグインが、heal のメトリクスをどのように Claude セッションに繋ぐか。
---

heal には Claude Code 用のプラグインが同梱されています。これにより、
heal が収集するメトリクスを Claude セッションに自動で流し込めます。
プラグインは `heal skills install` でリポジトリごとに一度だけインス
トールします。それ以降:

- Claude のすべての編集とターン終了がログに記録されます。
- メトリクスがしきい値を越えると、次の Claude セッションの冒頭に通
  知が表示されます。
- 5 つの `check-*` スキルが利用可能になり、特定メトリクスについて
  Claude にオンデマンドで尋ねられます。

## インストール

```sh
heal skills install
```

これでプラグインツリーが `.claude/plugins/heal/` に展開されます。

```
.claude/plugins/heal/
├── plugin.json
├── hooks/
│   ├── claude-post-tool-use.sh
│   ├── claude-stop.sh
│   └── claude-session-start.sh
└── skills/
    ├── check-overview/SKILL.md
    ├── check-hotspots/SKILL.md
    ├── check-complexity/SKILL.md
    ├── check-duplication/SKILL.md
    └── check-coupling/SKILL.md
```

プラグインツリーはコンパイル時に `heal` バイナリに埋め込まれている
ため、インストールされるバージョンは常にバイナリと一致します。
`heal` をアップグレードした後は `heal skills update` でリフレッシュ
してください。

## フックがすること

プラグインには 3 つのフックが同梱されています。3 つともすべて、同
じ `heal hook` エントリポイントに戻って呼び出します。

| フックイベント | 振る舞い                                                                 |
| -------------- | ------------------------------------------------------------------------ |
| `PostToolUse`  | Claude による Edit / Write / MultiEdit を `.heal/logs/` に記録します。   |
| `Stop`         | Claude のターン終了をログに記録します。                                  |
| `SessionStart` | 直近のスナップショット差分を読み、しきい値を超えていれば通知を出します。 |

最初の 2 つは純粋なログ記録です — オブザーバーを動かさないので、
Claude のターンに測定可能なレイテンシは追加しません。

3 つの中でもっとも影響が大きいのは `SessionStart` です。Claude セッ
ションが開くと、次の処理を行います。

1. `.heal/snapshots/` から最新の `MetricsSnapshot` をロード。
2. ひとつ前のスナップショットと比較。
3. 5 つのルール（new top hotspot、new top CCN function、new top
   Cognitive function、CCN spike、duplication growth）を評価。
4. 発火しかつクールダウン（デフォルト 24 時間）が切れているルール
   について、Claude がセッション冒頭で見る Markdown 通知を出力。
5. 同じルールがクールダウン期限まで再発火しないよう
   `.heal/runtime/state.json` を更新。

クールダウンはルールごとなので、別の侵害があれば次のセッションで待
たずに発火できます。

## 5 つの `check-*` スキル

特定の `heal status --metric <X>` 呼び出しをラップする、リードオン
リーの Claude スキルです。

| スキル              | 機能                                                                   |
| ------------------- | ---------------------------------------------------------------------- |
| `check-overview`    | 有効な全メトリクスを 1 つの状況レポートに統合。                        |
| `check-hotspots`    | Hotspot ランキングに踏み込み、各上位ファイルがなぜスコアしたかを解説。 |
| `check-complexity`  | worst CCN / Cognitive 関数を辿り、リファクタ候補を提示。               |
| `check-duplication` | 重複ブロックをレビューし、ヘルパーを抽出できそうな箇所を提案。         |
| `check-coupling`    | 共変ペアをレビューし、抽象が欠けていそうな箇所を提案。                 |

スキルは 2 通りで呼び出せます。

- ターミナルから: `heal check hotspots` — Claude をヘッドレスモード
  （`claude -p`）で起動。
- インタラクティブな Claude セッション内: Claude に
  `check-hotspots` スキルを使うよう指示。

5 つのスキルはすべてリードオンリーです。`heal status` を呼ぶことは
できますが、ソースファイルは変更しません。修復スキル（`run-*`）は
将来追加予定です。

## プラグインの更新

`heal` バイナリをアップグレードした後:

```sh
heal skills update
```

これは**ドリフトを意識**します。heal はインストールしたファイルそ
れぞれのフィンガープリントを `.claude/plugins/heal/.heal-install.json`
に記録します。更新時:

- 記録された同梱フィンガープリントと一致するファイルは、新しい同梱
  バージョンで上書きされます。
- フィンガープリントが異なるファイル（手動編集されたもの）は警告と
  ともに残されます。
- `--force` を渡すと手動編集も含めてすべて上書きします。

`heal skills status` は、ドリフトしたファイルを並べて比較表示します。

## 削除

```sh
heal skills uninstall
```

`.claude/plugins/heal/` を削除します。それ以外は触りません。`.heal/`
配下のプロジェクトデータはそのまま残ります。

## なぜ同梱なのか

`cargo install heal-cli` という単一の配信チャネルが、CLI と対応プ
ラグインを同時に提供します。バージョンを揃えてリリースすることで、
プラグインとバイナリのバージョンミスマッチを防ぎます。トレードオフ
は、プラグインが `heal` バイナリと同じ鮮度であるという点です。スキ
ルプロンプトを独立に書き換えたい場合は `.claude/plugins/heal/` を
手で編集してください — `heal skills update` 時にそれらがドリフトと
してマークされる前提で。
