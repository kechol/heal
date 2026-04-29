---
title: Installation
description: Three ways to install the heal CLI — Homebrew, Cargo, or the shell installer.
---

heal is a single binary named `heal`. The three install methods
below produce the same binary; choose whichever suits the
environment.

## Requirements

- **OS**: macOS or Linux. Windows is not supported in v0.1; the hook
  scripts and path handling assume a POSIX shell.
- **Git**: any modern release. heal uses libgit2 internally, but you
  also need a working `git` CLI for the post-commit hook to fire.

## Homebrew (macOS / Linux)

```sh
brew install kechol/tap/heal-cli
```

This taps `kechol/homebrew-tap` and installs the prebuilt `heal`
binary that ships with each release. Upgrade with the usual
`brew upgrade`.

## Cargo

If you already have a Rust toolchain on `PATH` (1.85 or newer):

```sh
cargo install heal-cli
```

`cargo install` builds from crates.io and drops `heal` in
`~/.cargo/bin`. Make sure that directory is on your `PATH`.

## Shell installer (pre-built binary)

For installations without Homebrew or Rust:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/kechol/heal/releases/latest/download/heal-cli-installer.sh | sh
```

The script downloads the appropriate pre-built binary for the host
platform from the [latest GitHub release](https://github.com/kechol/heal/releases/latest)
and installs it under `$CARGO_HOME/bin` (defaults to `~/.cargo/bin`).
The artifact is identical to the one Homebrew uses, delivered
without the `brew` workflow.

## Verify the install

```sh
heal --version
heal --help
```

`heal --help` lists every subcommand. If the command is not found,
verify that `~/.cargo/bin` (or a custom `CARGO_HOME/bin`) is on the
shell `PATH`.

## Updating

| Install method | Update command                 |
| -------------- | ------------------------------ |
| Homebrew       | `brew upgrade heal-cli`        |
| Cargo          | `cargo install heal-cli` again |
| Shell          | re-run the installer command   |

After upgrading, run `heal skills update` inside any project that has
the Claude plugin installed, so the bundled skills stay in sync with
the binary.

## Uninstall

| Install method | Uninstall command          |
| -------------- | -------------------------- |
| Homebrew       | `brew uninstall heal-cli`  |
| Cargo          | `cargo uninstall heal-cli` |
| Shell          | `rm ~/.cargo/bin/heal`     |

`heal` writes only inside `.heal/` and the `.git/hooks/post-commit`
hook of repositories where `heal init` was run. Remove these
manually for a clean slate.
