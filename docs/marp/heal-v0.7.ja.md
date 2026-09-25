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
    align-content: start;
  }
  section.lead {
    text-align: center;
    align-content: center;
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
    padding: 16px 24px 8px;
    margin: 12px 0 0;
    background: transparent;
    letter-spacing: 0.02em;
  }
  .plugin-cmd {
    font-family: 'SF Mono', 'Menlo', 'Consolas', monospace;
    font-size: 0.95em;
    color: #334155;
    text-align: center;
    line-height: 1.7;
    margin: 12px 0 20px;
  }
  .plugin-cmd .plugin-label {
    display: block;
    font-family: 'Helvetica Neue', 'Hiragino Sans', sans-serif;
    font-size: 0.8em;
    color: #64748b;
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
  .flow-h.tight {
    margin-top: 4px;
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
  .flow-step-heal {
    background: #f0fdfa;
    color: #0f766e;
    border-color: #99f6e4;
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
  .lanes {
    display: flex;
    flex-direction: column;
    gap: 14px;
    margin-top: 18px;
  }
  .lane {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .lane-label {
    width: 140px;
    flex-shrink: 0;
    font-weight: bold;
    color: #0f766e;
    font-size: 0.85em;
    line-height: 1.3;
  }
  .lane-label span {
    display: block;
    font-size: 0.8em;
    font-weight: normal;
    color: #64748b;
  }
  .lane .flow-step {
    font-size: 0.78em;
    padding: 8px 12px;
  }
  .arch-notes {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 22px;
  }
  .arch-layer {
    background: #f8fafc;
    border-left: 4px solid #0f766e;
    border-radius: 6px;
    padding: 8px 16px;
    font-size: 0.8em;
    line-height: 1.4;
  }
  .arch-layer-title {
    font-weight: bold;
    color: #0f766e;
  }
  .arch-layer-detail {
    color: #475569;
  }
  .fam-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 24px;
    margin-top: 14px;
  }
  .fam-card {
    background: #f8fafc;
    border-top: 4px solid #0f766e;
    border-radius: 6px;
    padding: 12px 20px 14px;
    font-size: 0.74em;
    line-height: 1.55;
  }
  .fam-card h3 {
    margin: 0 0 2px;
    color: #0f766e;
    font-size: 1.3em;
  }
  .fam-card .fam-q {
    color: #475569;
    font-style: italic;
    margin: 0 0 8px;
  }
  .fam-card ul {
    margin: 0;
    padding-left: 1.2em;
  }
  .fam-note {
    text-align: center;
    font-size: 0.78em;
    color: #475569;
    margin-top: 16px;
  }
  .sem-grid {
    display: grid;
    grid-template-columns: 1.35fr 1fr;
    gap: 28px;
    align-items: start;
  }
  .sem-grid table {
    font-size: 0.72em;
    margin-top: 0;
  }
  .sem-grid td,
  .sem-grid th {
    padding: 6px 10px;
  }
  .sem-grid td:first-child {
    width: 64%;
  }
  .sem-uses {
    font-size: 0.76em;
    line-height: 1.55;
  }
  .sem-uses h4 {
    margin: 12px 0 6px;
    color: #0f766e;
  }
  .sem-uses ul {
    margin: 0;
    padding-left: 1.2em;
  }
  .note {
    font-size: 0.74em;
    color: #475569;
    margin-top: 14px;
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

`heal` は git リポジトリで動く **CLI** です。`heal init` で1度セットアップすれば、コミットのたびに観測が走り、Critical / High の項目が `git commit` の出力に表示されます。`heal status` を実行すると、優先度つきの TODO リストが出ます。

その TODO リストを読んで実際に直すのは、heal プラグインの **Claude Skill** です。`/heal:refactor` が改善案を出し、承認された提案を1つずつコミットしていきます。

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

heal が動くのは、コミットした時と、自分で呼んだ時の2通りだけです。常駐するデーモンやバックグラウンドプロセスはありません。

<div class="lanes">
  <div class="lane">
    <div class="lane-label">コミット時<span>自動</span></div>
    <div class="flow-step">git commit</div>
    <div class="flow-arrow">→</div>
    <div class="flow-step">Hook</div>
    <div class="flow-arrow">→</div>
    <div class="flow-step">Observe</div>
    <div class="flow-arrow">→</div>
    <div class="flow-step">Classify</div>
    <div class="flow-arrow">→</div>
    <div class="flow-step flow-step-end">Critical / High を表示</div>
  </div>
  <div class="lane">
    <div class="lane-label">呼んだ時<span>手動</span></div>
    <div class="flow-step">heal status</div>
    <div class="flow-arrow">→</div>
    <div class="flow-step">Observe</div>
    <div class="flow-arrow">→</div>
    <div class="flow-step">Classify</div>
    <div class="flow-arrow">→</div>
    <div class="flow-step">Cache</div>
    <div class="flow-arrow">→</div>
    <div class="flow-step flow-step-heal">Skill が提案・修正</div>
  </div>
</div>

<div class="arch-notes">
  <div class="arch-layer">
    <span class="arch-layer-title">Observe</span> <span class="arch-layer-detail">— tree-sitter で AST を、git2 でコミット履歴を解析する</span>
  </div>
  <div class="arch-layer">
    <span class="arch-layer-title">Classify</span> <span class="arch-layer-detail">— コードベース自身の分布（calibration）から Severity と Hotspot を判定する</span>
  </div>
  <div class="arch-layer">
    <span class="arch-layer-title">Cache</span> <span class="arch-layer-detail">— TODO リストを <code>.heal/findings/latest.json</code> に保存する。このファイルも git で管理するので、同じコミットならチーム全員が同じ TODO リストを見る</span>
  </div>
</div>

---

## CLI: 指標の分析結果を、優先度つきで出力

```text
$ heal status
  HEAD 405a282  (4990 findings)
  Drain queue: T0 19 findings (9 files)  ·  T1 95 findings (23 files)

═══ Code ═══
🔴 Critical 🔥 [T0 Must drain] (19)
  crates/cli/src/core/config.rs  coupled (sym)
  crates/cli/src/observers.rs    CCN=42
  crates/cli/src/cli.rs          CCN=27
  crates/cli/src/feature.rs      LCOM=6
  ...
  Next: `claude /heal:refactor` drains this family's T0 queue
```

分析結果は、**直す順番の段（Tier）→ Severity → Hotspot スコア** の順に並びます。先頭の T0 は「Critical かつ 🔥 Hotspot」、つまり真っ先に直すべき場所です。

Severity のしきい値は、**そのコードベース自身の分布で calibration** されます。プロジェクトの規模に依らず、相対的に妥当な優先度が付きます。

---

## Skill: 改善案を出し、承認したものだけを直す

```text
$ claude /heal:refactor
Reading
  複雑度が2つのファイルに集中し、どちらも Critical かつ Hotspot です。
Proposals  (T0: 5 shown of 7)
  [1] src/payments/engine.ts を pricing と validation に分ける   risk: medium
      Why:       価格計算を変えるたびに、40行の入力チェックを読み直している
      Pattern:   Extract Class
      Evidence:  LCOM 2 clusters · CCN 28 · 🔥 Hotspot
Accept instead
  - ccn  src/parser/table.rs:dispatch — exhaustive_enum_dispatch
```

<div class="flow-h tight">
  <div class="flow-step">診断</div>
  <div class="flow-arrow">→</div>
  <div class="flow-step">提案</div>
  <div class="flow-arrow">→</div>
  <div class="flow-step">承認</div>
  <div class="flow-arrow">→</div>
  <div class="flow-step flow-step-heal">1提案1コミットで適用</div>
</div>

TODO リスト全体を見渡して、**どこを、どう分ければ変えやすくなるか** を提案します。2つの指標が同じ切れ目を指していれば、ファイルの分割のような構造的な変更にも踏み込みます。適用するのは **承認した提案だけ** で、1提案ごとにテストを通してからコミットします。

---

## Test / Docs: テストとドキュメントにも同じループを

`.heal/config.toml` で追加できる指標のまとまり（ファミリ）です。Code と同じく、TODO リストに並んだ項目を専用の Skill が直します。

<div class="fam-grid">
  <div class="fam-card">
    <h3>Test <code>[features.test]</code></h3>
    <p class="fam-q">テストから見えていない本番コードはどこか？</p>
    <ul>
      <li><b>入力:</b> カバレッジ計測ツールが出す <code>lcov.info</code></li>
      <li><b>指標:</b> <code>coverage_pct</code>（行カバレッジ）、<code>skip_ratio</code>（skip されたテスト）、<code>change_coupling.drift</code>（ソースに取り残されたテスト）</li>
      <li><b>🔥 Test Hotspot:</b> 変更頻度 × 未カバー率</li>
      <li><b>Skill:</b> <code>/heal:tests</code> が足りないテストを書き、ずれたテストを直す</li>
    </ul>
  </div>
  <div class="fam-card">
    <h3>Docs <code>[features.docs]</code></h3>
    <p class="fam-q">ドキュメントは実装からずれていないか？</p>
    <ul>
      <li><b>入力:</b> ドキュメントとソースの対応表 <code>.heal/doc_pairs.json</code></li>
      <li><b>指標:</b> <code>doc_drift</code>（存在しない識別子の参照）、<code>doc_freshness</code>（ソースに追いついていない）、リンク切れ、孤立ページ、TODO の密度</li>
      <li><b>🔥 Doc Hotspot:</b> ソースの変更頻度 × ドキュメントの遅れ</li>
      <li><b>Skill:</b> <code>/heal:docs</code> がリンク切れや参照切れを直し、ずれた説明を書き直す</li>
    </ul>
  </div>
</div>

<div class="fam-note"><code>/heal:setup tests</code> / <code>/heal:setup docs</code> で、有効化から準備までを済ませられます。</div>

---

## Semantic: 指標では測れない「意味」を問う

指標で分かるのは「どこが変えにくいか」までで、「名前と中身が合っているか」は分かりません。`[features.semantic]` を有効にすると、こうした問いを TypeSafe の分類モデル **Jev** に投げ、確率つきの答えを TODO リストに加えます。

<div class="sem-grid">
  <table>
    <thead><tr><th>問い</th><th>項目</th></tr></thead>
    <tbody>
      <tr><td>名前が中身と合っているか</td><td><code>name_mismatch</code></td></tr>
      <tr><td>1つのファイルに概念が混ざっていないか</td><td><code>concept_mix</code></td></tr>
      <tr><td>テストが振る舞いを確かめているか</td><td><code>test_value</code></td></tr>
      <tr><td>ドキュメントの説明がまだ正しいか</td><td><code>doc_drift.semantic</code></td></tr>
    </tbody>
  </table>
  <div class="sem-uses">
    <h4>答えの使い道</h4>
    <ul>
      <li><b>並び順:</b> 同じ Tier・Severity の中で、利用者への影響・変えにくさ・バグ修正の多さを使って並べ替える</li>
      <li><b>構造的な変更の根拠:</b> 指標と答えが同じ箇所を指せば、ファイルの分割などを提案する</li>
      <li><b>自己検証:</b> Skill が自分の提案とコミットを Jev に確かめさせる</li>
    </ul>
  </div>
</div>

<div class="note">コードを外部に送るのは <code>heal semantic ask</code> を実行した時だけです。答えは <code>.heal/semantic/verdicts/</code> に保存してコミットするので、API キーを持たないメンバーも同じ結果を見られます。</div>

---

<!-- _class: lead -->

<p class="cta-heading">ぜひ使ってみてください</p>

<div class="install-hero">brew install kechol/tap/heal-cli</div>

<div class="plugin-cmd">
  <span class="plugin-label">Claude Code で Skill を入れる</span>
  /plugin marketplace add kechol/heal<br>
  /plugin install heal@heal
</div>

<div class="links">

<img class="icon-inline" src="https://cdn.jsdelivr.net/gh/devicons/devicon@latest/icons/github/github-original.svg" alt="GitHub" /> https://github.com/kechol/heal
📖 https://kechol.github.io/heal/ja/

</div>

---

## Appendix 1: Code ファミリの指標

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
| **Test ファミリ**                     | `lcov.info` を出力できるすべての言語                          |
| **Docs ファミリ**                     | Markdown / RST のドキュメントと、上記6言語のソース            |

**対応言語:** 6言語の解析器はすべてリリースバイナリに同梱されていて、追加のインストールは要りません。

**モノレポ対応:** `[[project.workspaces]]` で workspace を宣言すれば、各 workspace を独立した分布でキャリブレーションできます。5kloc の CLI と 50kloc の API を、別々のしきい値で評価できます。
