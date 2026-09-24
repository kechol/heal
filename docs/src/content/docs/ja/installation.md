---
title: インストール
description: heal CLI をインストールする 3 つの方法 — Homebrew、Cargo、シェルインストーラー — と、スキルを入れる Claude Code プラグイン。
---

heal は `heal` という名前の単一バイナリです。以下の 3 つのインストール方法はいずれも同じバイナリを生成します。環境に合うものを選んでください。Claude のスキルは別に、Claude Code プラグインとして入れます（下の [Claude Code プラグイン](#claude-code-プラグイン) を参照）。

## 必要なもの

- **OS**: macOS または Linux。Windows は未対応 — フックスクリプトとパス処理は POSIX シェルを前提にしています。
- **Git**: モダンな任意のリリース。heal は内部で libgit2 を使いますが、post-commit フックを発火させるためには `git` CLI も動作する必要があります。

## Homebrew（macOS / Linux）

```sh
brew install kechol/tap/heal-cli
```

`kechol/homebrew-tap` を tap し、各リリースに同梱されるビルド済みの `heal` バイナリをインストールします。アップグレードは通常通り `brew upgrade` です。

## Cargo

`PATH` に Rust ツールチェーン（1.90 以上）がすでに通っている場合:

```sh
cargo install heal-cli
```

`cargo install` は crates.io からビルドし、`heal` を `~/.cargo/bin` に配置します。このディレクトリが `PATH` に含まれていることを確認してください。

## シェルインストーラー（ビルド済みバイナリ）

Homebrew も Rust もない環境向け:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/kechol/heal/releases/latest/download/heal-cli-installer.sh | sh
```

スクリプトは [GitHub の最新リリース](https://github.com/kechol/heal/releases/latest) からホストプラットフォーム向けのビルド済みバイナリをダウンロードし、`$CARGO_HOME/bin`（デフォルト `~/.cargo/bin`）に配置します。配信物は Homebrew が使うものと同一で、`brew` のワークフローを介さない経路です。

## インストールを確認

```sh
heal --version
heal --help
```

`heal --help` で全サブコマンドが列挙されます。コマンドが見つからない場合は、`~/.cargo/bin`（または独自の `CARGO_HOME/bin`）がシェルの `PATH` に通っているか確認してください。

## Claude Code プラグイン

スキル（`/heal:setup`・`/heal:refactor`・`/heal:docs`・`/heal:tests`）は、heal リポジトリから Claude Code プラグインとして配布しています。Claude Code で次を実行します。

```
/plugin marketplace add kechol/heal
/plugin install heal@heal
```

プラグインはプロジェクトではなく Claude Code の設定側に入るので、git 管理下のファイルは増えません。プラグインは公開時の heal リリースに固定されています。heal を使っているプロジェクトでセッションを始めると、`heal` CLI が同じリリースのものかを確かめ、違っていれば更新コマンドを 1 行だけ表示します。

スキルは Claude Code 向けに書かれています。Codex CLI を使う場合は、[heal リポジトリ](https://github.com/kechol/heal) の `plugins/heal/skills/` 以下のフォルダを、プロジェクトの `.agents/skills/` に手でコピーすれば使えます。ただしサポート対象外です。

## アップデート

| インストール方法 | 更新コマンド                      |
| ---------------- | --------------------------------- |
| Homebrew         | `brew upgrade heal-cli`           |
| Cargo            | `cargo install heal-cli` を再実行 |
| Shell            | インストーラーコマンドを再実行    |

CLI をアップグレードしたら、スキルが合うようにプラグインも更新してください。Claude Code で `/plugin marketplace update heal` を実行してから `/plugin update heal@heal` を実行します。

### heal 0.6 以前からのアップグレード

以前のバージョンは、スキルを各プロジェクトにコピーしていました（`.claude/skills/heal-*`、`.agents/skills/heal-*`）。プラグインを入れたら、各プロジェクトで次を実行してコピーを消し、削除をコミットしてください。

```sh
heal skills uninstall
```

消すのは heal 自身が書いたフォルダだけで、自分で作ったスキルは残ります。残っているものは `heal doctor` が一覧にします。

## アンインストール

| インストール方法 | アンインストールコマンド   |
| ---------------- | -------------------------- |
| Homebrew         | `brew uninstall heal-cli`  |
| Cargo            | `cargo uninstall heal-cli` |
| Shell            | `rm ~/.cargo/bin/heal`     |

`heal` が書き込みを行うのは、`heal init` を実行したリポジトリ内の `.heal/` と `.git/hooks/post-commit` フックだけです。完全に消したい場合は手動で削除してください。プラグインは Claude Code で `/plugin uninstall heal@heal` を実行すると外れます。
