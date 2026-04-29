---
title: Concept
description: Why HEAL exists, what problem it solves, and how it thinks about your codebase.
---

This page explains the _why_ behind HEAL. If you just want to start
using it, jump to [Getting Started](/heal/getting-started/) — you can
come back later.

## The problem

AI coding agents like Claude Code are great at writing the next thing
you tell them to write. But your codebase is always changing too: every
time you fix a bug or add a feature, complexity creeps in, hot files
get hotter, and copy-pasted blocks pile up. **The agent does not know
that. It waits for you to notice.**

That is fine for a small project. On a real codebase, you end up
firefighting — by the time you remember "wait, that file got messy
six months ago", the mess is already in production.

## The HEAL idea

> **Turn codebase health signals into agent triggers.**

HEAL watches your codebase the same way a CI system watches your
tests:

- Every commit, it measures the codebase (complexity, churn,
  duplication, hotspots).
- It writes a small snapshot to `.heal/snapshots/`.
- The next time you open a Claude Code session, HEAL surfaces what
  changed — _if_ something crossed a threshold worth your attention.

So instead of you remembering to run a linter, the agent gets a
heads-up: _"the function you just touched in `init.rs` is now the
worst CCN in the project — want to do something about it?"_

## The loop

Long-term, HEAL is built around a three-step loop:

1. **Observe** — collect health metrics on every commit.
2. **Nudge** — surface meaningful changes to the human + agent at the
   right moment (session start).
3. **Repair** — let the agent open a PR that fixes the highlighted
   issue, with policy gates on what is allowed to merge.

**v0.1 (where you are now) ships steps 1 and 2.** Step 3 — the
"autonomous repair" half — lands in v0.2 behind opt-in policy. You can
read more under [Architecture](/heal/architecture/).

## Read-only by default

HEAL is **not** going to silently rewrite your code. Every command in
v0.1 either reads metrics or pipes them to Claude for explanation. The
only files HEAL writes are:

- `.heal/` — its own data directory
- `.git/hooks/post-commit` — a one-line shim, installed once
- `.claude/plugins/heal/` — when you opt in via `heal skills install`

The `propose` and `execute` rungs of the policy ladder become
meaningful in v0.2; until then, every fix is a human decision after
reading what HEAL surfaced.

## Why metrics, not vibes

Six metrics ship in v0.1:

- **LOC** — what languages dominate the project
- **Complexity (CCN + Cognitive)** — which functions are hard to follow
- **Churn** — which files change a lot
- **Change Coupling** — which files always change together
- **Duplication** — which code blocks got copy-pasted
- **Hotspot** — churn × complexity, the classic "code as a crime
  scene" view

Each one is a _boring_, well-studied number. None of them are AI-
specific. The point of HEAL is not the metrics themselves — they
have been around for decades — but **using them as triggers for the
agent loop**, so the human does not have to remember to check.

For the math behind each metric, see [Metrics](/heal/metrics/).

## Why hook-driven

Agents are smart at generating code, but bad at remembering to look
around. Hooks make the codebase _talk back_:

- The **git post-commit hook** writes a snapshot the moment a commit
  lands. No daemon, no schedule, no polling.
- The **Claude Code SessionStart hook** reads the latest snapshot the
  moment a session opens. The agent gets the signal exactly when it
  is about to act.

Both hooks call the same `heal` binary. There is no background
process and nothing to manage.

## What HEAL is _not_

- **Not a linter.** Linters say "this line is bad". HEAL says "this
  _file_ is interesting".
- **Not a code reviewer.** That is Claude's job; HEAL shapes the
  prompt.
- **Not a CI gate.** The post-commit hook fires _after_ you commit.
  HEAL is about the long-term arc of the codebase, not blocking a PR.
- **Not multi-agent (yet).** v0.1 is Claude Code only. A provider
  abstraction lands in v0.5.

## Where to go next

- [Getting Started](/heal/getting-started/) — install and try it on a
  real repo
- [Metrics](/heal/metrics/) — what each number means
- [CLI](/heal/cli/) — every command you will use
- [Architecture](/heal/architecture/) — how the pieces fit together
