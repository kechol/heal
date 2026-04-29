---
name: check-overview
description: First-read health summary for someone unfamiliar with the codebase — synthesizes the dominant signals across every metric, frames them with the established literature, and proposes a concrete first step. Trigger on "what does this repo look like?", "I'm new here", "give me a health report". Read-only — proposes follow-ups without applying them.
---

# check-overview

Orientation skill. The goal is to give the user a clear sense of what
state the codebase is in and *what to do next*, without assuming they
know the file structure. Lead with intent and impact; numbers support
the narrative, they don't replace it.

## What the metrics tell you

HEAL composes signals from the maintainability literature:

- **Hotspot** (Tornhill, *Your Code as a Crime Scene*) = files that
  are both **frequently changed** and **complex**. Empirically, these
  account for the majority of post-release defects despite being a
  small fraction of the codebase. Almost always the highest-leverage
  starting point.
- **Cyclomatic Complexity / CCN** (McCabe, 1976) = independent paths
  through a function. Maps to the minimum number of test cases for
  branch coverage. CCN ≥ 10 is the "needs review" threshold;
  ≥ 20 is "high risk"; ≥ 50 is "untestable in practice".
- **Cognitive Complexity** (Sonar, 2017) = how hard a function is to
  *understand* — penalises nesting, rewards linear flow. Threshold
  ~15 review, ~25 refactor priority.
- **Duplication** (DRY, Hunt & Thomas) = same intent in multiple
  places. Type-1 (verbatim, ≥ 50 tokens) is the conservative signal
  HEAL ships; coincidental similarity (license headers, generated
  fixtures) is filtered out by the threshold.
- **Change coupling** (Gall et al., popularised by Tornhill) = files
  that ship in the same commit repeatedly. Cross-module pairs without
  a static dependency are a hidden architectural seam.

## Procedure

1. Run `heal status` (text mode, no `--metric`). For any metric where
   you want concrete top entries, follow up with
   `heal status --metric <name> --json` and read `worst.*` (the top-N
   precomputed using each metric's `top_n` from `.heal/config.toml`).
2. Compose a 3-part summary, capped at ~30 lines total:
   - **At a glance** (1 sentence): primary language, rough size,
     `snapshots` count. Note if no snapshot exists yet — the user
     needs at least one commit to get delta data.
   - **Top 3 concerns** (mixed across metrics, the most actionable).
     Per concern: one plain-language sentence about the file/function,
     one sentence about *why it matters in this codebase's context*
     (review burden, bug risk, duplicated maintenance, hidden
     dependency), and the supporting numbers in parentheses. Reach
     for the metric's literature — "high churn × high CCN means new
     bugs land here disproportionately" is more useful than "score 630".
   - **First step**: pick ONE concrete action and describe it
     specifically using Fowler's vocabulary where it fits — Extract
     Function, Replace Conditional with Polymorphism, Strangler Fig,
     etc. ("Open `src/foo.rs:120`. `parse_args` has CCN 22 driven by
     a 5-deep `if` chain; pulling the input-validation block into a
     helper would drop CCN to ~12 — Extract Function.")
3. End with two offers:
   - "Want me to drill into one of these? See `check-hotspots`,
     `check-complexity`, `check-duplication`, `check-coupling` for
     focused analysis."
   - "Want me to draft the refactor? `run-code-*` skills will
     automate this in v0.2."
4. If a `delta vs prior snapshot` block exists, lead with the biggest
   movement — that's the freshest signal and usually the most
   actionable, since prevention is cheaper than cure.

## When the project is brand-new

If `snapshots: 0` and `snapshot_segments: 0`, the user hasn't committed
since `heal init`. Say so plainly: "no snapshot data yet — make a
commit and the post-commit hook will produce the first record."

## Constraints

- Read-only at the file level. You may **read** any flagged file to
  ground your explanation; do not edit.
- Plain language before numbers. "This file accumulates bug fixes
  because every feature touches it" is more useful than "score 630.0".
- Cap at ~30 lines synthesis. Detailed drill-down belongs in the
  per-metric `check-*` skills — name them and offer to invoke.
- Do not duplicate the per-metric skills' deep analysis; this is the
  hub, not the destination.
