---
title: インストール
description: heal CLI を入れる 3 つの方法（Homebrew、Cargo、シェルインストーラー）と、スキルを収めた Claude Code プラグインの入れ方。
---

heal は `heal` という名前の 1 つのバイナリです。下の 3 つの方法のどれでも同じバイナリが入るので、環境に合うものを選んでください。Claude のスキルは別に、Claude Code プラグインとして入れます（[Claude Code プラグイン](#claude-code-プラグイン) を参照）。

## 必要なもの

- **OS**: macOS または Linux。フックのスクリプトとパスの扱いが POSIX シェルを前提にしているため、Windows には対応していません。
- **Git**: 最近のバージョンなら何でも構いません。heal は内部で libgit2 を使いますが、post-commit フックを動かすには `git` コマンドも必要です。

## Homebrew（macOS / Linux）

```sh
brew install kechol/tap/heal-cli
```

`kechol/homebrew-tap` を tap し、リリースごとに配布しているビルド済みの `heal` バイナリを入れます。更新はいつもの `brew upgrade` でできます。

## Cargo

Rust ツールチェーン（1.90 以降）が `PATH` にあれば、次のコマンドで入ります。

```sh
cargo install heal-cli
```

`cargo install` は crates.io のソースからビルドし、`heal` を `~/.cargo/bin` に置きます。このディレクトリが `PATH` に入っていることを確かめてください。

## シェルインストーラー（ビルド済みバイナリ）

Homebrew も Rust もない環境向けの方法です。

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/kechol/heal/releases/latest/download/heal-cli-installer.sh | sh
```

スクリプトは [GitHub の最新リリース](https://github.com/kechol/heal/releases/latest) から、実行中のプラットフォームに合うビルド済みバイナリを取得し、`$CARGO_HOME/bin`（既定は `~/.cargo/bin`）に置きます。中身は Homebrew で入るものと同じで、`brew` を使わずに届けるという違いだけです。

## インストールを確認

```sh
heal --version
heal --help
```

`heal --help` はすべてのサブコマンドを一覧表示します。コマンドが見つからないときは、`~/.cargo/bin`（`CARGO_HOME` を変えているならその下の `bin`）がシェルの `PATH` に入っているか確かめてください。

## Claude Code プラグイン

スキル（`/heal:setup`、`/heal:refactor`、`/heal:docs`、`/heal:tests`）は、heal のリポジトリから Claude Code プラグインとして配布しています。Claude Code で次を実行します。

```
/plugin marketplace add kechol/heal
/plugin install heal@heal
```

プラグインはプロジェクトではなく Claude Code の設定の側に入るので、git 管理下のファイルは増えません。プラグインは、公開したときの heal のリリースに固定されています。heal を使っているプロジェクトでセッションを始めるたびに、プラグインは `heal` CLI が同じリリースのものかを確かめ、違っていれば更新コマンドを 1 行で表示します。

スキルは Claude Code 向けに書いています。Codex CLI で使いたい場合は、[heal のリポジトリ](https://github.com/kechol/heal) にある `plugins/heal/skills/` 以下のフォルダを、プロジェクトの `.agents/skills/` に手でコピーすれば動きますが、この使い方はサポートしていません。

## アップデート

| インストール方法 | 更新コマンド                            |
| ---------------- | --------------------------------------- |
| Homebrew         | `brew upgrade heal-cli`                 |
| Cargo            | `cargo install heal-cli` をもう一度実行 |
| シェル           | インストーラーのコマンドをもう一度実行  |

CLI を更新したら、スキルと揃うようにプラグインも更新してください。Claude Code で `/plugin marketplace update heal` を実行してから、`/plugin update heal@heal` を実行します。

### heal 0.6 以前からのアップグレード

以前のバージョンは、スキルを各プロジェクトにコピーしていました（`.claude/skills/heal-*`、`.agents/skills/heal-*`）。プラグインを入れたら、各プロジェクトでそのコピーを消し、削除をコミットしてください。

```sh
heal skills uninstall
```

消すのは heal 自身が書いたフォルダだけで、自分で作ったスキルは残ります。消し残しがあれば `heal doctor` が一覧で示します。

## アンインストール

| インストール方法 | アンインストールコマンド   |
| ---------------- | -------------------------- |
| Homebrew         | `brew uninstall heal-cli`  |
| Cargo            | `cargo uninstall heal-cli` |
| シェル           | `rm ~/.cargo/bin/heal`     |

`heal` が書き込むのは、`heal init` を実行したリポジトリの `.heal/` と `.git/hooks/post-commit` フックだけです。まっさらな状態に戻したいときは、これらを手で消してください。プラグインは Claude Code で `/plugin uninstall heal@heal` を実行すると外れます。
