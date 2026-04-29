---
title: Concept
description: Why heal exists, what problem it solves, and how it approaches your codebase.
---

This page describes the design rationale behind heal. To start using
it directly, see [Quick Start](/heal/quick-start/) and return here
later.

## The problem

AI coding agents such as Claude Code are effective at producing the
next requested change. The codebase, meanwhile, evolves continuously:
each bug fix or feature accumulates complexity, the same files are
touched repeatedly, and duplicated blocks gradually appear. The agent
does not observe these long-term shifts — without an external signal,
the human is the only one tracking them.

For a small project, that is workable. On a real codebase the result
is reactive maintenance: by the time someone notices that a file has
become difficult to work with, the regressions are already in
production.

## The heal idea

> **Turn codebase health signals into agent triggers.**

heal observes the codebase in the same way a CI system observes test
runs:

- On every commit, it measures the codebase (complexity, churn,
  duplication, hotspots).
- It writes a small snapshot to `.heal/snapshots/`.
- When the next Claude Code session opens, heal surfaces what changed
  — provided something crossed a threshold worth attention.

The result: rather than relying on the human to remember to run a
linter, the agent receives a structured notice — for example,
_"the function recently modified in `init.rs` is now the highest CCN
in the project; consider refactoring."_

## The loop

Long-term, heal is structured around a three-step loop:

1. **Observe** — collect health metrics on every commit.
2. **Nudge** — surface meaningful changes to the human and agent at
   the relevant moment (session start).
3. **Repair** — allow the agent to open a PR that addresses the
   highlighted issue, with policy gates governing what may merge.

**v0.1 ships steps 1 and 2.** Step 3 — the autonomous repair half —
arrives in v0.2 behind an opt-in policy. See
[Architecture](/heal/architecture/) for details.

## Read-only by default

heal does not modify source files in v0.1. Every command either reads
metrics or hands them to Claude for explanation. The only files heal
writes are:

- `.heal/` — its own data directory
- `.git/hooks/post-commit` — a single hook line, installed once
- `.claude/plugins/heal/` — opt-in, via `heal skills install`

The `propose` and `execute` rungs of the policy ladder activate in
v0.2. Until then, every change is a human decision informed by what
heal surfaced.

## Why metrics

Six metrics ship in v0.1:

- **LOC** — language composition of the project
- **Complexity (CCN + Cognitive)** — functions that are difficult to
  follow
- **Churn** — files that change frequently
- **Change Coupling** — files that change together
- **Duplication** — code blocks that have been copied
- **Hotspot** — churn × complexity, the "code as a crime scene" view

Each is a long-standing, well-studied metric. None are AI-specific.
heal's contribution is not the metrics themselves — they have existed
for decades — but the use of them as triggers for the agent loop,
removing the human from the polling path.

For the formulas behind each metric, see [Metrics](/heal/metrics/).

## Why hook-driven

Agents produce code well but do not consistently inspect the
surrounding state. Hooks let the codebase emit signals on its own:

- The **git post-commit hook** writes a snapshot when a commit lands.
  No daemon, no schedule, no polling.
- The **Claude Code SessionStart hook** reads the latest snapshot when
  a session opens. The agent receives the signal at the moment it is
  about to act.

Both hooks invoke the same `heal` binary. There is no background
process to manage.

## What heal is not

- **Not a linter.** Linters report on individual lines. heal reports
  on which files warrant attention.
- **Not a code reviewer.** That role belongs to Claude; heal shapes
  the prompt.
- **Not a CI gate.** The post-commit hook fires after a commit lands.
  heal tracks the long-term trajectory of the codebase rather than
  blocking individual PRs.
- **Not multi-agent (yet).** v0.1 supports Claude Code only. A
  provider abstraction lands in v0.5.

## Further reading

- [Quick Start](/heal/quick-start/) — install and try it on a real
  repository
- [Metrics](/heal/metrics/) — what each metric means
- [CLI](/heal/cli/) — the full command surface
- [Architecture](/heal/architecture/) — how the components fit
  together
