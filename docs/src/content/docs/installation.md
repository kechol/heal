---
title: Installation
description: Three ways to install the heal CLI — Homebrew, Cargo, or the shell installer.
---

HEAL is a single binary called `heal`. Pick whichever install method
fits your environment — they all give you the same binary.

## Requirements

- **OS**: macOS or Linux. Windows is not supported in v0.1; the hook
  scripts and path handling assume a POSIX shell.
- **Git**: any modern release. HEAL uses libgit2 internally, but you
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

For one-off installs without Homebrew or Rust:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/kechol/heal/releases/latest/download/heal-cli-installer.sh | sh
```

The script downloads the right pre-built binary for your platform
from the [latest GitHub release](https://github.com/kechol/heal/releases/latest)
and installs it under `$CARGO_HOME/bin` (defaults to `~/.cargo/bin`).
It is the same artifact Homebrew uses — just delivered without the
`brew` ceremony.

## Verify the install

```sh
heal --version
heal --help
```

`heal --help` lists every subcommand. If the binary is missing,
double check that `~/.cargo/bin` (or your custom `CARGO_HOME/bin`)
is on your shell `PATH`.

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
hook of repositories where you ran `heal init`. Remove those by hand
if you want a clean slate.
