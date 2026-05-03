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

heal observes the codebase the same way a CI system observes test
runs:

- On every commit, it measures the codebase (complexity, churn,
  duplication, hotspots, LCOM).
- It surfaces every Critical / High Finding to stdout right inside
  the commit output.
- On demand (`heal status`), it classifies findings by Severity and
  writes a TODO list cache that the bundled `/heal-code-patch` skill can
  drain — one finding per commit.

The result: rather than relying on the human to remember to run a
linter, the agent receives a structured, prioritised list — and the
post-commit hook keeps the next move visible without needing a daemon
or polling.

## The loop

heal is structured around four steps: **observe → calibrate → check →
fix**.

```
Every commit
─────────────────────────────────────────────────

git commit
    │
    ▼
post-commit hook ──► heal hook commit
                          │
                          ├─ run observers (one pass)
                          │
                          └─ surface Critical / High to stdout

                    (nudge printed; nothing persisted)


On demand
─────────────────────────────────────────────────

heal status
    │
    ├─ classify findings via .heal/calibration.toml
    ├─ write CheckRecord ──► .heal/findings/latest.json
    ├─ reconcile fixed.json ↔ regressed.jsonl
    └─ render Severity-grouped view


claude /heal-code-patch
    │
    └─ drain .heal/findings/latest.json one finding per commit
       (Severity order; Critical 🔥 first)
```

## Codebase-relative Severity

A naïve threshold ("CCN ≥ 10 is high") works poorly across projects:
a 200-line script and a 200kloc service operate in different worlds.
heal calibrates each metric to the **codebase's own distribution**:

- `p75 / p90 / p95` from the initial scan become the percentile
  breaks under the literature-derived absolute floor.
- Above the floor (or above `p95`): Critical.
- `≥ p90`: High.
- `≥ p75`: Medium.
- otherwise: Ok.

`Hotspot` is **orthogonal** — it's a flag (top-10% of the hotspot
score), not a Severity. A finding can be `Critical 🔥`,
`Critical`, `High 🔥`, etc. — the renderer surfaces them as
separate buckets.

`heal calibrate --force` rescans and overwrites
`.heal/calibration.toml`; without `--force` the command is a no-op
when the file already exists. Drift detection (calibration age,
codebase file count change, clean streak) lives in the `/heal-config`
skill, which compares `calibration.toml.meta` against the current
findings cache and recommends `heal calibrate --force` when warranted.
Recalibration is **never automatic** — the user controls when to
reset the baseline.

## Read-only by default; write through the skill

The `heal` CLI itself never modifies source files. Repair flows
through the bundled `/heal-code-patch` Claude skill, which:

- refuses to run on a dirty worktree,
- commits one finding per fix,
- never pushes,
- never amends.

`heal mark-fixed` is the single CLI subcommand that mutates state — it
records a `FixedFinding` in `fixed.json` and is meant to be called by
`/heal-code-patch` after each commit.

## Why metrics

Seven metrics ship with heal:

- **LOC** — language composition of the project
- **Complexity (CCN + Cognitive)** — functions difficult to follow
- **Churn** — files that change frequently
- **Change Coupling** — files that change together; both the
  one-way leader/follower count and the symmetric ("responsibility
  mixing") subset
- **Duplication** — code blocks that have been copied
- **Hotspot** — churn × complexity, the "code as a crime scene" view
- **LCOM** — classes whose methods don't share state (mechanically
  separable)

Each is a long-standing, well-studied metric. None are AI-specific.
heal's contribution is not the metrics themselves — they have existed
for decades — but using them as **calibrated triggers** for the agent
loop, removing the human from the polling path.

For the formulas behind each metric, see [Metrics](/heal/metrics/).

## Why hook-driven

Agents produce code well but do not consistently inspect the
surrounding state. Hooks let the codebase emit signals on its own.
The **git post-commit hook** runs every observer once and surfaces
the Severity nudge to stdout when a commit lands. No daemon, no
schedule, no persistent state. heal does not register any Claude
Code hooks — the loop runs entirely through the bundled skills.

## What heal is not

- **Not a linter.** Linters report on individual lines. heal reports
  on which files warrant attention and in what order.
- **Not a code reviewer.** That role belongs to Claude; heal shapes
  the prompt and the TODO list.
- **Not a CI gate.** The post-commit hook fires after a commit lands.
  heal tracks the long-term trajectory of the codebase rather than
  blocking individual PRs.
- **Not a replacement for tests.** heal surfaces structural
  complexity; correctness is still your test suite's job.

## Further reading

- [Quick Start](/heal/quick-start/) — install and try it on a real
  repository
- [Metrics](/heal/metrics/) — what each metric measures and how
  Severity is assigned
- [CLI](/heal/cli/) — the full command surface (`heal status`,
  `heal diff`, `heal metrics`, `heal calibrate`)
- [Configuration](/heal/configuration/) — `.heal/config.toml` and
  `.heal/calibration.toml` reference
- [Architecture](/heal/architecture/) — on-disk layout, event
  streams, the cache contract
- [Claude skills](/heal/claude-skills/) — `/heal-code-review`,
  `/heal-code-patch`, `/heal-cli`, and `/heal-config`
