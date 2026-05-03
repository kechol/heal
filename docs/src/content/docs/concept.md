---
title: Concept
description: Why heal exists, what problem it solves, and how it approaches your codebase.
---

This page explains the _why_. To start using heal directly, see
[Quick Start](/heal/quick-start/) and come back later.

## The problem

AI coding agents are great at the next change you ask for. But the
codebase keeps moving in the background: each fix or feature adds a
little complexity, the same files get touched over and over,
duplicated blocks slip in. The agent doesn't watch for that — and on
a real codebase, by the time _you_ notice that a file has become
hard to work with, the regressions are already shipping.

## The idea

> **Turn codebase health into agent triggers.**

Instead of asking the human to remember to run a linter, heal lets
the codebase emit signals on its own.

- **Every commit**, a post-commit hook re-runs every observer and
  prints any Critical / High finding right inside the commit output —
  the next problem stays visible without a daemon.
- **On demand**, `heal status` writes the same findings to a TODO
  cache that the bundled `/heal-code-patch` Claude skill drains, one
  fix per commit.

The result is a loop where the codebase wakes the agent up, rather
than waiting for the human to do so.

## Codebase-relative Severity

A naïve threshold ("CCN ≥ 10 is high") works poorly across projects
— a 200-line script and a 200kloc service operate in different
worlds. heal calibrates each metric to **your codebase's own
distribution**: the top decile of _your_ complexity becomes High, the
top 5% becomes Critical. Recalibration is manual (`heal calibrate
--force`) — a refactor that genuinely improves the codebase shouldn't
silently move the goalposts.

Two literature-grade absolute floors bracket the percentile
classifier so a uniformly-bad codebase still surfaces its worst
cases, and a uniformly-clean codebase isn't held hostage by the "top
10% is always red" loop. See [Metrics](/heal/metrics/) for the full
ladder.

## Why Hotspot matters

Of the seven metrics heal ships, **Hotspot is the one to watch
first**. A high-complexity file that nobody touches is technical
debt — interesting, but not urgent. A high-complexity file that
the team edits every other day is where the next bug ships from.
Hotspot multiplies churn × complexity to surface exactly those
files: high score = often touched **and** hard to read = where
regressions historically concentrate.

The `🔥` flag in `heal status` marks these. A `Critical 🔥` finding
isn't twice as bad as a `Critical` finding — it's the one that
actually pays back the time you spend fixing it. The `/heal-code-patch`
skill drains the `🔥` queue first by default for the same reason.

## Read-only by default; write through skills

The `heal` CLI itself never modifies source files. Repair flows
through two bundled Claude skills with deliberately split roles:

- **`/heal-code-review`** is the _thinking_ skill. Read-only. It
  reads the cache as a system, deep-reads the flagged code, and
  proposes architectural moves — the calls a human still has to make.
  Use this when you want to _understand_ what the cache is telling you
  before changing anything.
- **`/heal-code-patch`** is the _doing_ skill. Mechanical only — it
  drains the cache one finding per commit using established refactor
  patterns whose application doesn't require domain judgement. Refuses
  to start on a dirty worktree, never pushes, never amends. When the
  next finding needs an architectural decision, it stops and hands
  back to `/heal-code-review`.

This split is the contract that lets you trust autonomy. The boring
fixes happen on their own; the interesting calls stay with you.

## What heal is not

- **Not a linter.** Linters report on individual lines. heal reports
  on which files warrant attention and in what order.
- **Not a code reviewer.** That role belongs to Claude;
  `/heal-code-review` orchestrates it. heal shapes the prompt and the
  TODO list.
- **Not a CI gate.** The post-commit hook fires _after_ a commit
  lands. heal tracks the long-term trajectory of the codebase rather
  than blocking individual PRs.
- **Not a replacement for tests.** heal surfaces structural
  complexity; correctness is still your test suite's job.

## Further reading

- [Quick Start](/heal/quick-start/) — install and try it on a real
  repository
- [Metrics](/heal/metrics/) — what each metric measures and how
  Severity is assigned
- [CLI](/heal/cli/) — every subcommand
- [Configuration](/heal/configuration/) — `.heal/config.toml` and
  `.heal/calibration.toml` reference
- [Claude skills](/heal/claude-skills/) — `/heal-code-review`,
  `/heal-code-patch`, `/heal-cli`, and `/heal-config`
