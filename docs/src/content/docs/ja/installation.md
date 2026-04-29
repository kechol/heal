---
title: インストール
description: heal CLI をインストールする 3 つの方法 — Homebrew、Cargo、シェルインストーラー。
---

heal は `heal` という名前の単一バイナリです。以下の 3 つのインストー
ル方法はいずれも同じバイナリを生成します。環境に合うものを選んでく
ださい。

## 必要なもの

- **OS**: macOS または Linux。Windows は未対応 — フックスクリプトと
  パス処理は POSIX シェルを前提にしています。
- **Git**: モダンな任意のリリース。heal は内部で libgit2 を使います
  が、post-commit フックを発火させるためには `git` CLI も動作する
  必要があります。

## Homebrew（macOS / Linux）

```sh
brew install kechol/tap/heal-cli
```

`kechol/homebrew-tap` を tap し、各リリースに同梱されるビルド済み
の `heal` バイナリをインストールします。アップグレードは通常通り
`brew upgrade` です。

## Cargo

`PATH` に Rust ツールチェーン（1.85 以上）がすでに通っている場合:

```sh
cargo install heal-cli
```

`cargo install` は crates.io からビルドし、`heal` を `~/.cargo/bin`
に配置します。このディレクトリが `PATH` に含まれていることを確認し
てください。

## シェルインストーラー（ビルド済みバイナリ）

Homebrew も Rust もない環境向け:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/kechol/heal/releases/latest/download/heal-cli-installer.sh | sh
```

スクリプトは [GitHub の最新リリース](https://github.com/kechol/heal/releases/latest)
からホストプラットフォーム向けのビルド済みバイナリをダウンロードし、
`$CARGO_HOME/bin`（デフォルト `~/.cargo/bin`）に配置します。配信物
は Homebrew が使うものと同一で、`brew` のワークフローを介さない経路
です。

## インストールを確認

```sh
heal --version
heal --help
```

`heal --help` で全サブコマンドが列挙されます。コマンドが見つからな
い場合は、`~/.cargo/bin`（または独自の `CARGO_HOME/bin`）がシェル
の `PATH` に通っているか確認してください。

## アップデート

| インストール方法 | 更新コマンド                      |
| ---------------- | --------------------------------- |
| Homebrew         | `brew upgrade heal-cli`           |
| Cargo            | `cargo install heal-cli` を再実行 |
| Shell            | インストーラーコマンドを再実行    |

アップグレード後は、Claude プラグインを入れているプロジェクトで
`heal skills update` を実行し、同梱スキルをバイナリに追従させてく
ださい。

## アンインストール

| インストール方法 | アンインストールコマンド   |
| ---------------- | -------------------------- |
| Homebrew         | `brew uninstall heal-cli`  |
| Cargo            | `cargo uninstall heal-cli` |
| Shell            | `rm ~/.cargo/bin/heal`     |

`heal` が書き込みを行うのは、`heal init` を実行したリポジトリ内の
`.heal/` と `.git/hooks/post-commit` フックだけです。完全に消したい
場合は手動で削除してください。
