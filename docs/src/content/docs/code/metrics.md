---
title: Code · Metrics
description: The seven Code-family metrics, the Severity ladder, and why Hotspot is the one to watch first.
---

The Code family ships seven metrics. None are AI-specific — each
is a long-standing code-health signal with decades of literature
behind it. heal's contribution is **calibrating them to your
codebase's own distribution**: a 200-line script and a 200kloc
service trigger differently for the same raw value.

## Severity ladder

Every Finding gets `Critical`, `High`, `Medium`, or `Ok`. Two
stages, in order:

1. **Absolute floors** (literature-anchored): `value ≥ floor_critical`
   → Critical; `value < floor_ok` → Ok. Keeps the worst cases red on
   a uniformly-bad codebase, and lets a clean codebase graduate off
   the percentile loop.
2. **Codebase percentiles**, between the floors: `≥ p95` Critical,
   `≥ p90` High, `≥ p75` Medium, otherwise Ok.

Percentile breaks live in `.heal/calibration.toml` (rebuilt by
`heal calibrate`). Floors are config-overridable per metric — see
[Configuration](/heal/code/configuration/).

## Drain tiers

`heal status` groups every non-Ok Finding into three tiers driven
by `[policy.drain]`:

- **T0 — Drain queue** (default `["critical:hotspot"]`) — the
  must-fix list `/heal-code-patch` works through.
- **T1 — Should drain** (default `["critical", "high:hotspot"]`)
  — surfaced separately, not auto-drained.
- **Advisory** — anything else above Ok. Hidden unless `--all`.

T0 is the goal, T1 is hygiene, Advisory is review-when-convenient.

## The metrics

### LOC — Lines of Code

> _"What does this codebase consist of?"_

Code, comment, and blank lines per language via
[`tokei`](https://github.com/XAMPPRocky/tokei). Other metrics
depend on its language detection. Markdown / Org are excluded from
primary-language detection so a docs-heavy repo still resolves to
its implementation language. Always on, no Severity.

### Complexity — CCN and Cognitive

> _"Which functions are difficult to follow?"_

Two per-function metrics computed in one pass:

- **CCN** (Cyclomatic Complexity) — McCabe's branch count.
- **Cognitive Complexity** — Sonar's readability metric, penalising
  nesting depth.

Both calibrate independently with literature-anchored floors
(McCabe for CCN, SonarQube for Cognitive). They are **proxy
metrics**: floor `floor_ok` graduates clean codebases off the
ladder.

**Languages**: TypeScript / JavaScript / Python / Go / Scala / Rust.

### Churn — how often a file changes

> _"What is moving?"_

Per-file commit count over the last `since_days` window (default
90), first-parent only. Churn has no Severity of its own — it
feeds Hotspot and the post-commit nudge.

### Change Coupling — files that move together

> _"Which files implicitly depend on each other?"_

Every commit's touched files form a co-occurrence event; per-pair
counters reveal dependencies the import graph doesn't show. Pairs
are classified **Symmetric** (both change together, strongest
signal) or **OneWay**.

Pairs that look like coupling but aren't — lockfile bumps,
generated code, `mod.rs ↔ sibling` — are dropped automatically.
Test ↔ source and doc ↔ source pairs are demoted to Advisory by
default; when `[features.test]` is on, drifting test pairs are
re-promoted as `change_coupling.drift` (see
[Test › Metrics](/heal/test/metrics/)).

### Duplication — copied blocks

> _"Where are the duplicates?"_

Long runs of identical tokens (Type-1 clones), found by walking
the parse tree with a sliding window of `min_tokens` (default 50).
Reformatting doesn't hide a clone; renaming a variable does.

When `[features.docs]` is on, a parallel pass runs over Markdown /
RST files — see [Docs › Metrics](/heal/docs/metrics/).

**Languages**: same as Complexity.

### LCOM — Lack of Cohesion of Methods

> _"Which classes are mechanically separable?"_

Per class, heal builds a graph: methods that share field
references or call each other are connected. The number of
connected components is the LCOM value; `cluster_count ≥ 2` means
the class has separable concerns and is a candidate for Extract
Class.

The current syntactic backend has known blind spots (inheritance,
dynamic property access). Treat surfaced classes as candidates for
human review, not auto-decisions.

**Languages**: TypeScript / JavaScript / Python / Rust class
scopes. (Go has no class scope; Scala awaits the LSP backend.)

## Hotspot

Hotspot multiplies churn × complexity to surface the files that
are both hard to read and frequently edited. The output is a
per-file flag (top-10% of the score distribution), rendered as
the `🔥` emoji on top of any other Finding for that file — so a
finding can be `Critical 🔥`, `High 🔥`, `Medium 🔥`, or even
`Ok 🔥`.

A complex file nobody touches is debt; a complex file the team
edits every other day is where the next bug ships from. The
default drain queue `(critical:hotspot)` is exactly that
intersection — that's why Hotspot is the **single most actionable
signal** heal produces.

The "Ok 🔥" subset — heavily touched but no Severity finding —
shows up in its own section under `heal status --all` ("why are we
still editing this?" candidates).

For the longer rationale see
[Concept › Hotspot](/heal/concept/#hotspot--the-file-most-likely-to-break-next).
