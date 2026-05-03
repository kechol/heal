---
marp: true
theme: default
paginate: true
size: 16:9
header: 'heal — Turn codebase health into agent triggers'
style: |
  section {
    font-family: 'Helvetica Neue', 'Hiragino Sans', sans-serif;
    padding: 60px 80px;
  }
  section.lead {
    text-align: center;
    justify-content: center;
  }
  h1 {
    color: #0f766e;
  }
  h2 {
    color: #0f766e;
    border-bottom: 2px solid #0f766e;
    padding-bottom: 8px;
  }
  strong {
    color: #0f766e;
  }
  code {
    background: #f1f5f9;
    color: #0f766e;
    padding: 2px 6px;
    border-radius: 4px;
  }
  pre {
    background: #0f172a;
    color: #e2e8f0;
    border-radius: 6px;
    padding: 16px;
    font-size: 0.75em;
  }
  pre code {
    background: transparent;
    color: inherit;
  }
  header {
    color: #94a3b8;
    font-size: 0.7em;
  }
  .acronym {
    font-size: 0.85em;
    color: #475569;
    line-height: 1.9;
  }
  .acronym b {
    color: #0f766e;
    font-size: 1.2em;
  }
  .links {
    font-size: 1.1em;
    line-height: 2.2;
  }
  .icon-inline {
    width: 24px;
    height: 24px;
    vertical-align: -6px;
    margin-right: 8px;
  }
  .install-hero {
    font-family: 'SF Mono', 'Menlo', 'Consolas', monospace;
    font-size: 1.7em;
    font-weight: bold;
    color: #0f766e;
    text-align: center;
    padding: 32px 24px;
    margin: 24px 0;
    background: transparent;
    letter-spacing: 0.02em;
  }
  .cta-heading {
    font-size: 1.1em;
    color: #475569;
    text-align: center;
    margin: 0 0 16px;
    font-weight: normal;
  }
  table {
    border-collapse: collapse;
    width: 100%;
    font-size: 0.78em;
    margin-top: 12px;
  }
  th, td {
    border: 1px solid #cbd5e1;
    padding: 8px 12px;
    text-align: left;
    vertical-align: top;
  }
  th {
    background: #0f766e;
    color: white;
  }
  tbody tr:nth-child(even) {
    background: #f1f5f9;
  }
  td code {
    font-size: 0.92em;
  }
  .flow-h {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: center;
    gap: 8px;
    margin-top: 28px;
    flex-wrap: wrap;
  }
  .flow-step {
    padding: 10px 16px;
    border-radius: 8px;
    background: #f1f5f9;
    color: #334155;
    border: 1px solid #cbd5e1;
    text-align: center;
    font-weight: 600;
    font-size: 0.9em;
    white-space: nowrap;
  }
  .flow-step-end {
    background: #fef2f2;
    color: #991b1b;
    border-color: #fecaca;
  }
  .flow-arrow {
    font-size: 1.3em;
    color: #94a3b8;
    line-height: 1;
  }
  .flow-loop-back {
    text-align: center;
    margin-top: 14px;
    font-size: 0.85em;
    color: #64748b;
    font-style: italic;
  }
  .loop-row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    font-size: 0.85em;
  }
  .loop-row .step {
    padding: 10px 16px;
    border-radius: 6px;
    color: white;
    white-space: nowrap;
    font-weight: bold;
  }
  .label-add { color: #1d4ed8; }
  .label-sub-color { color: #0f766e; }
  .dual-loops {
    display: grid;
    grid-template-columns: auto auto;
    justify-content: center;
    gap: 56px;
    align-items: start;
    margin-top: 16px;
  }
  .loop-center-col {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }
  .loop-codebase-with-arrows {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .loop-h-arrow {
    font-size: 1.6em;
    color: #475569;
    font-weight: bold;
    line-height: 1;
  }
  .loop-center-cycle {
    text-align: center;
    font-size: 0.78em;
    color: #475569;
    font-style: italic;
    line-height: 1.4;
  }
  .loop-side {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }
  .loop-side-title {
    font-weight: bold;
    text-align: center;
    font-size: 1em;
    margin-bottom: 6px;
    line-height: 1.2;
  }
  .loop-side-title .loop-side-sub {
    display: block;
    font-size: 0.7em;
    font-weight: normal;
    color: #64748b;
    margin-top: 2px;
  }
  .loop-step-box {
    padding: 10px 18px;
    border-radius: 6px;
    color: white;
    font-weight: bold;
    font-size: 0.9em;
    text-align: center;
    white-space: nowrap;
  }
  .loop-step-dev  { background: #3b82f6; }
  .loop-step-heal { background: #0f766e; }
  .loop-vert-arrow {
    font-size: 1.3em;
    color: #64748b;
    line-height: 1;
  }
  .loop-converge {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
    margin-top: 12px;
  }
  .loop-converge-arrow {
    font-size: 1.6em;
    color: #64748b;
  }
  .loop-codebase-box {
    background: #0f172a;
    color: white;
    padding: 14px 28px;
    border-radius: 10px;
    text-align: center;
    font-weight: bold;
    font-size: 1em;
  }
  .loop-cycle-note {
    text-align: center;
    margin-top: 10px;
    color: #475569;
    font-style: italic;
    font-size: 0.85em;
  }
  .hotspot-formula {
    text-align: center;
    font-size: 1.1em;
    font-weight: bold;
    color: #0f766e;
    margin: 14px 0;
    padding: 10px 14px;
    background: #f1f5f9;
    border-radius: 6px;
    font-family: 'SF Mono', 'Menlo', monospace;
  }
  .arch-stack {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 14px;
  }
  .arch-layer {
    background: #f8fafc;
    border-left: 4px solid #0f766e;
    border-radius: 6px;
    padding: 10px 16px;
    font-size: 0.9em;
    line-height: 1.4;
  }
  .arch-layer-title {
    font-weight: bold;
    color: #0f766e;
  }
  .arch-layer-detail {
    color: #475569;
  }
  .arch-arrow-up {
    text-align: center;
    font-size: 0.9em;
    color: #94a3b8;
    line-height: 1;
    margin: 0;
  }
  .step-dev    { background: #3b82f6; }
  .step-heal   { background: #0f766e; }
  .step-commit { background: #0f172a; }
  .loop-divider {
    text-align: center;
    margin: 12px 0;
    color: #475569;
    font-size: 0.85em;
    font-weight: bold;
  }
  .loop-back {
    text-align: center;
    margin-top: 14px;
    color: #475569;
    font-style: italic;
    font-size: 0.85em;
  }
---

<!-- _class: lead -->
<!-- _paginate: false -->
<!-- _header: '' -->

# AIが書いたコードの健全性を<br>保つためのCLI `heal`

<br>

### Turn codebase health into agent triggers

<br>

AIが書いたコードを、AIに直してもらう。そのためのCLIです。

---

## AIエージェントとコードの劣化

AIエージェントでコードを書くのが、当たり前になりました。機能追加のループは、これまでにないスピードで回っています。

一方で、AIは目の前のタスクをこなすのは得意でも、コードベース全体への配慮は不得意です。

- 似たコードを量産しがちで、**重複** が静かに増えていきます
- 局所的な動作を優先するため、**全体設計の整合性** が崩れていきます

その帰結として、コードベース全体は次のような道をたどります。

<div class="flow-h">
  <div class="flow-step">機能追加</div>
  <div class="flow-arrow">→</div>
  <div class="flow-step">複雑性が増す</div>
  <div class="flow-arrow">→</div>
  <div class="flow-step flow-step-end">不具合の温床に</div>
</div>
<div class="flow-loop-back">↺ 放置すると、ずっと繰り返されます</div>

---

## コードの健全性を測る指標は、昔から存在します

「**複雑な関数**」「**変更が集中するファイル**」「**重複したコード**」「**凝集度の低いクラス**」…

こうしたコードの傷んだ箇所を測る指標は、何十年も前から研究されてきました。経験のあるエンジニアがリファクタの判断に使ってきた指標です。

<br>

人の開発で機能してきた指標は、**AIが書いたコードベースにもそのまま使えるはずです**。

---

<!-- _class: lead -->

# そこで `heal` を作りました

<br>

### コードの劣化を、シグナルに。<br>シグナルを、AIのタスクに。

<br>

<div class="acronym">

HEAL = <b>H</b>ook-driven <b>E</b>valuation & <b>A</b>utonomous <b>L</b>oop

</div>

---

## コミットを起点に、改善が回り出す

`heal` は git リポジトリで動く **CLI** です。`heal init` で1度セットアップすれば、コミットのたびに観測が走り、優先度つきの分析結果がキャッシュされます。

その分析結果を読んで実際に修正するのは、同梱の **Claude Skill** です。`/heal-code-review` がアーキテクチャを読み解き、`/heal-code-patch` が1コミット1修正で消化していきます。

<div class="dual-loops">
  <div class="loop-side">
    <div class="loop-side-title label-add">＋ 増やすループ<span class="loop-side-sub">コード量 ↑</span></div>
    <div class="loop-step-box loop-step-dev">要望</div>
    <div class="loop-vert-arrow">↓</div>
    <div class="loop-step-box loop-step-dev">AI 実装</div>
  </div>
  <div class="loop-side">
    <div class="loop-side-title label-sub-color">− 減らすループ<span class="loop-side-sub">複雑性 ↓</span></div>
    <div class="loop-step-box loop-step-heal">heal シグナル</div>
    <div class="loop-vert-arrow">↓</div>
    <div class="loop-step-box loop-step-heal">Skill で修正</div>
  </div>
</div>

<div class="loop-cycle-note">↺ コミットのたびに、両方のループが回り続けます</div>

---

## Hotspot: もっとも重要な健全性指標

Hotspot とは、**複雑** で、かつ **頻繁に変更されている** ファイルのことです。

<div class="hotspot-formula">Hotspot = 複雑度 × 変更頻度</div>

- 複雑だけど、誰も触らないファイル → 気にはなりますが、急ぎません
- シンプルで、よく変更されるファイル → 問題ありません
- **複雑で、よく触られているファイル** → ここが次のバグの発生源です

開発者が変更のたびに「ここ、どうなってるんだっけ」と迷う場所ほど、ミスが入り込みやすくなります。Hotspot は、その「迷いやすい場所」をデータで特定する指標です。

直すなら、まずここから。**コードベース全体の健全化への近道です**。

---

## heal の内部アーキテクチャ

**①〜④** はコミットごとに自動で動き、**⑤** はユーザーが呼んだ時に動きます。

<div class="arch-stack">
  <div class="arch-layer">
    <span class="arch-layer-title">① Hook</span> <span class="arch-layer-detail">— git の post-commit フックが heal を起動する</span>
  </div>
  <div class="arch-arrow-up">↓</div>
  <div class="arch-layer">
    <span class="arch-layer-title">② Observe</span> <span class="arch-layer-detail">— tree-sitter で AST を解析、git2 で履歴を解析</span>
  </div>
  <div class="arch-arrow-up">↓</div>
  <div class="arch-layer">
    <span class="arch-layer-title">③ Classify</span> <span class="arch-layer-detail">— Severity と Hotspot を判定する</span>
  </div>
  <div class="arch-arrow-up">↓</div>
  <div class="arch-layer">
    <span class="arch-layer-title">④ Cache</span> <span class="arch-layer-detail">— 分析結果をキャッシュに保存する</span>
  </div>
  <div class="arch-arrow-up">— ここまでが自動 / ここから手動 —</div>
  <div class="arch-layer">
    <span class="arch-layer-title">⑤ Use</span> <span class="arch-layer-detail">— CLI が改善箇所を表示、Skill がレビューや修正を実行</span>
  </div>
</div>

---

## CLI: 指標の分析結果を、優先度つきで出力

```sh
$ heal status
  HEAD bd75d4a  (2179 findings)
  Drain queue: T0 9 findings  ·  T1 34 findings

🔴 Critical 🔥 [T0 Must drain] (9)
  crates/cli/src/commands/status.rs  CCN=32
  crates/cli/src/core/config.rs      coupled  LCOM=4
  crates/cli/src/commands/hook.rs    coupled  coupled
  ...

  Next: `claude /heal-code-patch` drains the T0 queue
```

各指標 (CCN、Coupling、LCOM 等) の分析結果は、Severity と Hotspot で並べ替えられて出力されます。コミットのたびに、自動で更新されていきます。

優先度の閾値は、**そのコードベース自身の分布で calibration** されます。プロジェクトの規模に依らず、相対的に妥当な優先度が付きます。

---

## Skill: アーキテクチャの改善案まで提示

```sh
$ claude /heal-code-review
```

```text
status.rs (CCN=32, 🔥 Hotspot)
  → render() に分岐が集中しています。Severity ごとに
    SectionRenderer を分けると、複雑度が下がります。

config.rs (LCOM=4, coupled)
  → load / validate / merge が同じ struct に同居しています。
    責務を分離すると、依存関係も解消できます。
```

指標から問題箇所を読み解き、**「どこを」「どう分割・整理すれば」健全になるか** を、具体的なリファクタ案として提示してくれます。

そのうち **機械的に修正できるもの** （関数の抽出、責務分離、重複の統合など）は `/heal-code-patch` が引き取り、1コミット1修正で実際にコードを書き換えていきます。**アーキテクチャ判断が必要なもの** は人の手元に残ります。

---

<!-- _class: lead -->

<p class="cta-heading">ぜひ使ってみてください</p>

<div class="install-hero">brew install kechol/tap/heal-cli</div>

<div class="links">

<img class="icon-inline" src="https://cdn.jsdelivr.net/gh/devicons/devicon@latest/icons/github/github-original.svg" alt="GitHub" /> https://github.com/kechol/heal
📖 https://kechol.github.io/heal/ja/

</div>

---

## Appendix 1: 計測している指標

| 指標                | 対象                       | 意味                     |
| ------------------- | -------------------------- | ------------------------ |
| **LOC**             | 言語ごとのコード行数       | コードベースの規模       |
| **CCN**             | 関数の分岐数 (McCabe)      | テストの難しさ           |
| **Cognitive**       | 関数の認知的複雑度 (Sonar) | コードの読みにくさ       |
| **Churn**           | ファイルの変更頻度         | 変更の集中度             |
| **Change Coupling** | 一緒に変更されるファイル   | ファイル間の暗黙の依存度 |
| **Duplication**     | コピペされたブロック       | 重複コードの多さ         |
| **LCOM**            | クラスの凝集度の欠如       | クラスの責務の分散度     |
| **Hotspot** 🔥      | 複雑度 × Churn             | バグが生まれやすい場所   |

---

## Appendix 2: 対応言語と構成

| 指標                                  | 対応言語                                                      |
| ------------------------------------- | ------------------------------------------------------------- |
| **LOC**                               | すべての言語に対応                                            |
| **Churn / Change Coupling / Hotspot** | すべての言語に対応                                            |
| **CCN / Cognitive / Duplication**     | TypeScript / JavaScript / Python / Go / Scala / Rust          |
| **LCOM**                              | TypeScript / JavaScript / Python / Rust (Go / Scala は未対応) |

<br>

**対応言語:** TypeScript / JavaScript / Python / Go / Scala / Rust の6言語をデフォルトでサポートしています。リリースバイナリにすべてバンドル済みです。

**モノレポ対応:** `[[project.workspaces]]` で workspace を宣言すれば、各 workspace を独立した分布でキャリブレーションできます。5kloc の CLI と 50kloc の API を、別々のしきい値で評価できます。
