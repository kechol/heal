---
title: Installation
description: Toolchain requirements and the supported install paths for the heal CLI.
---

HEAL is a Rust binary published through the `heal-cli` crate (the `heal`
crate name on crates.io is taken). Installing it gives you a single
`heal` executable on your `PATH`.

## Requirements

- **OS**: macOS or Linux. Windows is not supported in v0.1; the hook
  scripts and path handling assume a POSIX shell.
- **Rust toolchain**: 1.85 or newer. The repository pins the exact
  version via [mise](https://mise.jdx.dev) (`mise.toml`), but any rustup
  install at or above the minimum will work.
- **Git**: any modern release. `heal` depends on libgit2 (via the `git2`
  crate) for churn and change-coupling, but you also need a working
  `git` CLI for the post-commit hook to fire.

## From source (recommended for v0.1)

```sh
git clone https://github.com/kechol/heal
cd heal
cargo install --path crates/cli
```

`cargo install --path crates/cli` builds and copies the `heal` binary
into `~/.cargo/bin`. Make sure that directory is on your `PATH`.

## With mise

If you use [mise](https://mise.jdx.dev) for toolchain management, the
project ships a pinned `mise.toml`:

```sh
mise install
cargo install --path crates/cli
```

`mise install` reads the repo's `mise.toml` and brings the matching Rust
release into a project-local toolchain. From there `cargo install`
behaves exactly as above.

## crates.io

`cargo install heal-cli` is the planned distribution path. v0.1 is still
unreleased on crates.io; once the first tagged release lands, this will
be the one-line install. Follow [`TODO.md`](https://github.com/kechol/heal/blob/main/TODO.md)
for the release plan.

## Verifying the install

```sh
heal --version
heal --help
```

`heal --help` lists every subcommand. If the binary is missing, double
check that `~/.cargo/bin` (or your custom `CARGO_HOME/bin`) is on your
shell `PATH`.

## Uninstall

`cargo uninstall heal-cli` removes the binary. `heal` does not write
anywhere outside `.heal/` and `.git/hooks/post-commit` inside the
repositories where you ran `heal init`; remove those directories /
hook lines by hand if you want a clean slate.
