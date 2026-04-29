---
title: Getting Started
description: Five minutes to install HEAL, see your codebase health, and ask Claude what to focus on.
---

import { Tabs, TabItem } from '@astrojs/starlight/components';

By the end of this page you will have:

- HEAL running inside one of your git repositories
- The first metrics on screen — what your code actually looks like
- (Optional) Claude reading those metrics and pointing out what to
  focus on

No source clone, no manual build, no daemon. About **five minutes**.

## What you'll need

- macOS or Linux
- A git repository to play with — yours, or any open-source one you
  cloned earlier
- (Optional but fun) the [Claude Code](https://claude.com/code) CLI,
  if you want HEAL to also explain things in prose

## 1. Install `heal`

Pick whichever feels easiest:

<Tabs syncKey="installer">
  <TabItem label="Homebrew">

```sh
brew install kechol/tap/heal-cli
```

Works on macOS and Linux. Same prebuilt binary that ships with each
release.

  </TabItem>
  <TabItem label="Cargo">

```sh
cargo install heal-cli
```

If you already have a Rust toolchain (1.85+). Builds straight from
crates.io.

  </TabItem>
  <TabItem label="Shell">

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/kechol/heal/releases/latest/download/heal-cli-installer.sh | sh
```

No Rust, no Homebrew. Drops the prebuilt binary under
`~/.cargo/bin`.

  </TabItem>
</Tabs>

Quick sanity check:

```sh
heal --version
```

If you do not see a version, make sure `~/.cargo/bin` is on your
`PATH`. More on each method in [Installation](/heal/installation/).

## 2. Set up your repository

`cd` into any git repo and run:

```sh
heal init
```

That single command:

- Creates a `.heal/` directory to store metrics
- Installs a tiny `.git/hooks/post-commit` hook
- Captures the first snapshot

It is **safe to re-run** — your config and any existing hooks are
left alone unless you pass `--force`.

## 3. See what HEAL just measured

```sh
heal status
```

You will see something like this:

```
Primary language: Rust

LOC (top languages)
  Rust         18421 code   2891 comments
  TypeScript    4920 code    612 comments

Complexity (max CCN: 14, max Cognitive: 22)
  worst CCN:        run            crates/cli/src/commands/init.rs:78    14
  worst Cognitive:  collect        crates/cli/src/observer/churn.rs:42   22

Hotspots (top 3)
  crates/cli/src/commands/init.rs        score 482
  crates/cli/src/observer/loc.rs         score 213
  crates/cli/src/core/eventlog.rs        score 187

Δ since last commit
  max_ccn       +2  (run)
  hotspot top   unchanged
```

The bits to notice:

- **Primary language** comes from a quick `tokei` scan.
- **Hotspots** is the headline number — the file most worth looking
  at, scored as `commits × ccn_sum`.
- **Δ since last commit** is the _delta block_. A clean delta means
  your last commit did not make anything noticeably worse. A growing
  one is your early warning.

That is the entire feedback loop. Make a commit, run `heal status`,
read the delta.

## 4. (Optional) Ask Claude to explain it

If you have Claude Code installed:

```sh
heal check
```

This runs Claude in headless mode with a read-only skill that walks
through the metrics in plain language — what is hot, where the
duplicates are, which functions might want a refactor.

Want a focused conversation?

```sh
heal check hotspots       # zoom into the hotspot ranking
heal check complexity     # CCN and Cognitive worst-N
heal check duplication    # copy-paste blocks
heal check coupling       # files that always change together
```

`heal check` only **reads** your metrics. It does not modify your
code.

## 5. (Optional) Get nudges automatically

Steps 1–4 are on-demand: you run `heal status` when you want to
look. If you also want HEAL to **automatically** point things out at
the start of every Claude session, install the bundled plugin once:

```sh
heal skills install
```

From then on, when a metric crosses a threshold (a new top hotspot,
a CCN spike, growing duplication), the next Claude session opens
with a friendly heads-up at the top — no command from you, no
checking required.

You can install / update / remove the plugin any time with `heal
skills <subcommand>`. See [Claude plugin](/heal/claude-plugin/) for
how the hooks work.

## What to read next

- [Concept](/heal/concept/) — the design idea, in three minutes.
- [Metrics](/heal/metrics/) — what each number actually means.
- [CLI](/heal/cli/) — every subcommand with examples.
- [Configuration](/heal/configuration/) — tune thresholds for your
  project.

If anything in this walkthrough did not behave as described, that is
a bug — please [file an issue](https://github.com/kechol/heal/issues).
