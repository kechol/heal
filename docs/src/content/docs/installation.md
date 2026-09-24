---
title: Installation
description: Three ways to install the heal CLI — Homebrew, Cargo, or the shell installer — plus the Claude Code plugin that carries the skills.
---

heal is a single binary named `heal`. The three install methods
below produce the same binary; choose whichever suits the
environment. The Claude skills come separately, as a Claude Code
plugin (see [Claude Code plugin](#claude-code-plugin) below).

## Requirements

- **OS**: macOS or Linux. Windows is not supported; the hook scripts
  and path handling assume a POSIX shell.
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

If you already have a Rust toolchain on `PATH` (1.90 or newer):

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

## Claude Code plugin

The skills (`/heal:setup`, `/heal:refactor`, `/heal:docs`,
`/heal:tests`) ship as a Claude Code plugin from the heal repository.
In Claude Code, run:

```
/plugin marketplace add kechol/heal
/plugin install heal@heal
```

The plugin lives in your Claude Code settings, not in your project, so
nothing is added to your git tree. It is pinned to the heal release it
was published with. At the start of each session in a project that uses
heal, the plugin checks that the `heal` CLI comes from the same release
and prints one line with the upgrade command when it does not.

The skills are written for Claude Code. If you use Codex CLI, you can
copy the folders under `plugins/heal/skills/` from the
[heal repository](https://github.com/kechol/heal) into your project's
`.agents/skills/` by hand; that setup is not supported.

## Updating

| Install method | Update command                 |
| -------------- | ------------------------------ |
| Homebrew       | `brew upgrade heal-cli`        |
| Cargo          | `cargo install heal-cli` again |
| Shell          | re-run the installer command   |

After upgrading the CLI, update the plugin too so the skills match:
in Claude Code, run `/plugin marketplace update heal`, then
`/plugin update heal@heal`.

### Upgrading from heal 0.6 or earlier

Older versions copied the skills into each project
(`.claude/skills/heal-*`, `.agents/skills/heal-*`). After installing the
plugin, remove those copies from each project and commit the deletion:

```sh
heal skills uninstall
```

It removes only the folders heal itself wrote; your own skills stay.
`heal doctor` lists any that are left.

## Uninstall

| Install method | Uninstall command          |
| -------------- | -------------------------- |
| Homebrew       | `brew uninstall heal-cli`  |
| Cargo          | `cargo uninstall heal-cli` |
| Shell          | `rm ~/.cargo/bin/heal`     |

`heal` writes only inside `.heal/` and the `.git/hooks/post-commit`
hook of repositories where `heal init` was run. Remove these
manually for a clean slate. Remove the plugin with
`/plugin uninstall heal@heal` in Claude Code.
