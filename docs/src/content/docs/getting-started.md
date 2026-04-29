---
title: Getting Started
description: Install heal, set it up in a git repo, and see your first metrics in five minutes.
---

This page is a five-minute walkthrough. By the end you will have HEAL
running in a real repository and you will have read your first
metrics. If you want the _why_ before the _how_, start with
[Concept](/heal/concept/).

You will need:

- macOS or Linux (Windows is not supported in v0.1)
- A working git repo to play with
- Rust 1.85 or newer
- (Optional but recommended) the [Claude Code](https://claude.com/code) CLI

## 1. Install `heal`

```sh
git clone https://github.com/kechol/heal
cd heal
cargo install --path crates/cli
```

`cargo install` puts the `heal` binary in `~/.cargo/bin`. Make sure
that directory is on your `PATH`, then check:

```sh
heal --version
```

For full install options (mise, future crates.io release) see
[Installation](/heal/installation/).

## 2. Set up a repository

From inside any git repo:

```sh
heal init
```

This is the bootstrap step. It:

- Creates `.heal/` with a default `config.toml` and the
  `snapshots/` / `logs/` directories.
- Installs `.git/hooks/post-commit` (so every future commit writes a
  snapshot automatically).
- Runs the observers once and writes an initial snapshot.

`heal init` is **safe to re-run**. It will leave your config alone
unless you pass `--force`, and it marks its hook with a comment so
re-installing never duplicates.

## 3. (Optional) install the Claude plugin

If you use Claude Code, install the bundled plugin:

```sh
heal skills install
```

This drops a small plugin tree into `.claude/plugins/heal/` so that:

- Claude logs every edit it makes to `.heal/logs/`.
- The next time you open a Claude session in this repo, HEAL prints a
  nudge if a metric crossed a threshold since the last commit.

You can skip this step and HEAL still works — `heal status` and
`heal check` are both useful without the plugin. See
[Claude plugin](/heal/claude-plugin/) for what the plugin adds.

## 4. Make a commit and look at the metrics

Make any commit in the repo. After it lands:

```sh
heal status
```

You will see a summary of the enabled metrics — primary language,
recent churn, and so on — plus a delta block showing what moved
since the previous snapshot.

If you also have Claude Code installed:

```sh
heal check
```

This launches Claude headlessly with a read-only skill that walks
through the metrics in plain language. Claude does not modify your
code; it just explains what HEAL just measured.

## 5. Tune what you care about

The default config has the safe metrics on (LOC, churn, cognitive
complexity) and the heavier ones off (CCN, duplication, change
coupling, hotspot). Open `.heal/config.toml` and flip on what you
want:

```toml
[metrics.hotspot]
enabled = true

[metrics.duplication]
enabled = true
```

Save and run `heal status` again. There is no daemon to restart —
HEAL re-reads the config every time you call it.

For the full list of knobs see [Configuration](/heal/configuration/).

## What to read next

- [Concept](/heal/concept/) — the design idea, in three minutes.
- [Metrics](/heal/metrics/) — what each number actually means.
- [CLI](/heal/cli/) — every subcommand with examples.
- [Claude plugin](/heal/claude-plugin/) — hooks and the five
  `check-*` skills.
