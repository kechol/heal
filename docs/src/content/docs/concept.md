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

heal is structured around three steps:

1. **Observe** — collect health metrics on every commit.
2. **Nudge** — surface meaningful changes to the agent at the right moment.
3. **Repair** — allow the agent to open a pull request addressing the highlighted issue, with policy gates governing what may merge.

Here is how the first two steps play out in practice:

```
Every commit
─────────────────────────────────────────────────

git commit
    │
    ▼
post-commit hook ──► heal hook commit
                          │
                          ├─ run observers (LOC, complexity, churn, …)
                          │
                          └─ write snapshot ──► .heal/snapshots/

                    (snapshot stored, waiting)


Next Claude Code session
─────────────────────────────────────────────────

claude opens
    │
    ▼
SessionStart hook ──► heal hook session-start
                           │
                           ├─ load latest snapshot + delta
                           ├─ check thresholds and cool-down
                           │
                           ├─ threshold crossed ──► print nudge to Claude
                           └─ below threshold   ──► silent
```

**Cool-down** prevents the same notice from flooding every session.
After a rule fires, heal suppresses it for `cooldown_hours` (default
24 hours). The rule fires again if the threshold is still crossed once
the cool-down expires.

In practice, a cycle looks like this:

1. You commit a change; heal records a snapshot silently in the background.
2. You open Claude Code to continue working.
3. heal compares the new snapshot to the previous one.
4. If a hotspot file changed or complexity spiked, a brief notice
   appears at the top of Claude's context.
5. Claude can address it immediately, run `heal check` for details, or
   you can defer it.

The **Repair** step extends the loop further: rather than only
surfacing a notice, heal can open a pull request that addresses the
issue — gated by an explicit policy you control.

## Read-only by default

heal does not modify source files unless you enable a repair policy.
Every command either reads metrics or hands them to Claude for
explanation. The only files heal writes are:

- `.heal/` — its own data directory
- `.git/hooks/post-commit` — a single hook line, installed once
- `.claude/plugins/heal/` — opt-in, via `heal skills install`

Automated repair is gated behind an explicit `policy.action = execute`
setting. Until you set that, every change remains a human decision
informed by what heal surfaced.

## Why metrics

Six metrics ship with heal:

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
- **Not a replacement for tests.** heal surfaces structural complexity;
  correctness is still your test suite's job.

## Further reading

- [Quick Start](/heal/quick-start/) — install and try it on a real
  repository
- [Metrics](/heal/metrics/) — what each metric means
- [CLI](/heal/cli/) — the full command surface
- [Architecture](/heal/architecture/) — how the components fit
  together
