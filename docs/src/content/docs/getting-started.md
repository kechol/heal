---
title: Getting Started
description: Install the heal CLI, initialise it inside a git repository, and read your first metric snapshot.
---

This page walks through installing HEAL, wiring it into a git repository, and
reading the first snapshot it writes. It assumes a Unix shell (macOS or
Linux) and a working Rust toolchain (1.85+).

## 1. Install the CLI

```sh
git clone https://github.com/kechol/heal
cd heal
cargo install --path crates/cli
```

This produces a `heal` binary in `~/.cargo/bin`. The repository pins its
exact Rust toolchain via [mise](https://mise.jdx.dev) — running `mise install`
from the project root picks up the same version, but any rustup install
≥ 1.85 will work too.

## 2. Initialise inside a repository

From any git repository:

```sh
heal init
```

`heal init` creates `.heal/` (with `config.toml`, `snapshots/`, `logs/`,
`docs/`, `reports/`), installs `.git/hooks/post-commit`, and captures an
initial metric snapshot. The post-commit hook is idempotent: re-running
`heal init` leaves user-managed hook content alone unless you pass
`--force`.

The init step also detects whether you are on a **solo** or **team** project
based on distinct committer email count (≥ 3 → team) and writes a config
profile to match.

## 3. Install the Claude plugin

```sh
heal skills install
```

Extracts the bundled Claude Code plugin into `.claude/plugins/heal/`,
including hook scripts (`PostToolUse`, `Stop`, `SessionStart`) and five
read-only skills (`check-overview`, `check-hotspots`, `check-complexity`,
`check-duplication`, `check-coupling`).

Each installed file's fingerprint is recorded in `.heal-install.json` so
that `heal skills update` can refresh bundled assets without overwriting
files you have hand-edited.

## 4. Read the first snapshot

```sh
heal status
heal logs
```

`heal status` reads `.heal/snapshots/` and prints the metric summary plus
the most recent finding. `heal logs` streams the event log under
`.heal/logs/` (commit, edit, stop, session-start events). Both directories
share an append-only month-rotated JSONL format.

Once you make a commit, the post-commit hook calls `heal hook commit`,
which appends a fresh `MetricsSnapshot` to `snapshots/` and a lightweight
`CommitInfo` record to `logs/`. From that point onward `heal status` will
show a delta against the previous snapshot.

## 5. Where to go next

- Read about the [CLI commands](https://github.com/kechol/heal#cli) (a full
  per-command reference will land on this site soon).
- Skim the [TODO.md roadmap](https://github.com/kechol/heal/blob/main/TODO.md)
  to see what is in scope for v0.1 and what is deferred to v0.2+.
- Browse [`.heal/config.toml`](https://github.com/kechol/heal#configuration)
  options to tune thresholds for your project.
