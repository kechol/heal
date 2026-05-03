---
title: Metrics
description: The metrics heal collects on every commit, what each one means, how Severity is assigned, and when to use it.
---

heal ships seven metrics today. None are AI-specific; each is a
long-standing code-health metric that has been in use for decades.
heal's contribution is **calibrating them to the codebase's own
distribution** so a 200-line script and a 200kloc service trigger
differently for the same raw value, then surfacing the result as a
TODO list `/heal-code-patch` consumes.

This page summarises each metric. For configuration knobs, see
[Configuration](/heal/configuration/).

## Severity ladder

Every Finding lands on one of four tiers, evaluated in this order:

| Tier     | Rule                                                                                  |
| -------- | ------------------------------------------------------------------------------------- |
| Critical | `value ≥ floor_critical` (literature anchor — escape hatch for uniformly-bad codebases). |
| Ok       | `value < floor_ok` (literature anchor — graduation gate, **proxy metrics only**).        |
| Critical | `value ≥ p95` (the calibrated 95th percentile).                                       |
| High     | `value ≥ p90`                                                                         |
| Medium   | `value ≥ p75`                                                                         |
| Ok       | otherwise                                                                             |

Two absolute floors bracket the percentile classifier:

- `floor_critical` is the upper escape hatch — anything above it stays
  Critical even on a uniformly-bad codebase. Defaults from McCabe /
  SonarQube literature: CCN 25, Cognitive 50, Duplication 30%.
- `floor_ok` is the lower graduation gate — anything below it is Ok
  regardless of percentile, so a clean codebase isn't held hostage by
  the "top 10% is always red" loop (Goodhart's Law). Defaults: CCN 11
  (McCabe "simple, low risk"), Cognitive 8 (Sonar). Only applies to
  proxy metrics; duplication / change_coupling / lcom rely on their
  scan-time filters instead.

The percentile breaks (`p75 / p90 / p95`) come from the codebase's own
distribution at calibration time and live in `.heal/calibration.toml`.
Both floors are config-overridable per metric — see
[Configuration › Floors](/heal/configuration/#floors).

**Hotspot is orthogonal.** It is a flag (top-10% of the hotspot score
distribution), not a Severity. A finding can be `Critical 🔥`
(structurally bad AND being touched a lot) or `Critical` (structurally
bad, quiet) — the renderer surfaces them as separate buckets.

## Drain tiers

`heal check` groups non-Ok findings into three drain tiers driven by
the `[policy.drain]` config:

- **T0 — Drain queue** (default `["critical:hotspot"]`). The must-fix
  list `/heal-code-patch` drains.
- **T1 — Should drain** (default `["critical", "high:hotspot"]`).
  Bandwidth-permitting; surfaced separately, not auto-drained.
- **Advisory** — anything else above Ok. Hidden unless `--all`.

The split is what makes "drain to zero" meaningful: T0 is the goal,
T1 is hygiene, Advisory is review-when-convenient. CCN as a *proxy*
metric belongs in T0 only when corroborated by hotspot — otherwise the
metric drives a Goodhart loop. See
[Configuration › Drain policy](/heal/configuration/#drain-policy).

## Why CCN and Cognitive are *proxies*, not targets

McCabe (1976) introduced CCN as a static estimate of the minimum number
of test cases needed for branch coverage — not as a code-quality
metric. Sonar's Cognitive Complexity (2017) is a readability proxy.
Driving either toward zero damages readability:

- Extract Function on a procedurally cohesive function relocates CCN
  rather than reducing global count.
- Converting flat positive composites (`if (A && B && C)`) to negative
  guard chains doesn't move Cognitive (the original isn't nested) and
  often *increases* reader load.

heal's design accepts this: `floor_ok` graduates clean codebases off
the proxy metrics, hotspot multiplies leverage on touched files, and
the drain-tier model keeps the TODO list focused on findings where the
proxy and the underlying problem agree. See the
`heal-code-review` skill's `architecture.md` §6 for the trap catalogue.

## Recommended adoption order

1. **LOC** is always on; primary-language detection drives every
   other observer.
2. **CCN** + **Cognitive** are enabled by default. Calibration sets
   per-codebase thresholds on top of the absolute floors.
3. **Churn** is enabled. Becomes informative after roughly ten
   commits in the lookback window.
4. **Hotspot** is the most actionable single signal — leave on.
5. **Change Coupling** (one-way + symmetric) and **Duplication** are
   diagnostic. Leave them on; review when investigating a problem.
6. **LCOM** flags classes that are mechanically separable. Useful
   for refactor candidates; leave on.

## LOC — Lines of Code

> _"What does this codebase consist of?"_

Counts code, comment, and blank lines per language using
[`tokei`](https://github.com/XAMPPRocky/tokei). LOC is foundational:
other metrics depend on its language detection (complexity only runs
on languages it can parse, hotspot weights commits by complexity).

The "primary language" is the non-literate language with the most
code lines. Markdown and Org are deliberately excluded so a
documentation-heavy repository still resolves to its implementation
language.

LOC is always enabled; there is no toggle. The cost is negligible
because `tokei` caches per file.

## Complexity — CCN and Cognitive

> _"Which functions are difficult to follow?"_

Two per-function metrics, computed in a single tree-sitter walk:

- **CCN** (Cyclomatic Complexity) — the McCabe count of branches.
  Each `if`, `for`, `while`, `case`, `&&`, `||`, `?` adds one.
- **Cognitive Complexity** — Sonar's readability metric. Penalises
  nesting depth (each level adds increasingly more) and collapses
  chained logical operators into a single increment.

Both metrics calibrate independently:

- CCN: `floor_critical = 25` (McCabe "untestable"), `floor_ok = 11`
  (McCabe "simple").
- Cognitive: `floor_critical = 50` (SonarQube Critical baseline),
  `floor_ok = 8` (Sonar — half of the "review" threshold).

Functions strictly below `floor_ok` classify as Ok regardless of where
they land on the project's percentile ladder. This is the graduation
gate that lets a uniformly-clean codebase produce zero findings on a
proxy metric — without it the percentile classifier always flagged the
top decile (Goodhart's Law).

**Languages**: TypeScript and Rust. JS / Python / Go / Scala arrive
in later releases.

## Churn — how often a file changes

> _"What is moving?"_

Per-file commit count and added/deleted line totals over the last
`since_days` window (default 90), using first-parent history so
merge commits are not double-counted.

A high-churn file is not inherently problematic — `package.json`
changes frequently and that's fine. Churn becomes meaningful when
crossed with complexity (see [Hotspot](#hotspot--churn--complexity)).

Churn does not have its own Severity ladder; it feeds Hotspot and
the post-commit nudge.

## Change Coupling — files that move together

> _"Which files depend on which, implicitly?"_

For every commit, the set of paths it touches becomes one
co-occurrence event. Per-pair counters reveal implicit dependencies
that the import graph does not show. Pairs surviving `min_coupling`
(default 3) become Findings.

Beyond the raw counter, every pair is also classified as
**Symmetric** or **OneWay**:

- **Symmetric**: `min(P(B|A), P(A|B)) ≥ symmetric_threshold` (default
  0.5). Both files rarely change without the other — the strongest
  "responsibility mixing" signal in the metric.
- **OneWay { from, to }**: `from` changes alone often; `to` almost
  always tags along. Picked as the file the partner is more
  conditionally bound to.

Symmetric pairs surface under the metric tag
`change_coupling.symmetric`; the renderer separates them so they're
visible as a stronger signal than the generic counter.

**Bulk-commit cap**: commits touching more than 50 files are skipped
entirely so mass reformats can't fabricate coupling between
unrelated files.

## Duplication — copied blocks

> _"Where are the duplicates?"_

Finds long runs of identical tokens (Type-1 clones) by walking the
tree-sitter parse tree and matching token windows of size
`min_tokens` (default 50). Reformatting and whitespace changes do
not hide a clone; renaming a variable does.

Calibration uses the per-file duplicate-percentage distribution;
`floor_critical = 30%` (a third of the file is duplicate is a
structural problem).

**Languages**: same as complexity (TypeScript, Rust).

## Hotspot — churn × complexity

> _"Where do regressions concentrate?"_

The "code as a crime scene" view, popularised by Adam Tornhill.
Hotspot multiplies a file's commit count (churn) by the sum of its
functions' CCN (complexity):

```
score = (weight_complexity × ccn_sum) × (weight_churn × commits)
```

Files with a high score are both changed frequently and difficult
to read — historically where regressions concentrate.

The weights default to `1.0`. Hotspot uses an independent percentile
space — `score ≥ p90` flips the **flag**, which the renderer surfaces
as the `🔥` emoji on top of any other finding for that file. It is
**not** a Severity tier on its own; a hotspot file can be Critical 🔥,
High 🔥, Medium 🔥, or even Ok 🔥. The last group — low Severity but
heavily touched, "why are we still editing this?" candidates —
appears in a dedicated section under `heal check --all`.

The formula is multiplicative, so a file with high complexity but no
recent commits scores zero — hotspot is meant to identify _active_
trouble, not historical debt.

## LCOM — Lack of Cohesion of Methods

> _"Which classes are mechanically separable?"_

Per class (TS `class_declaration`, Rust `impl_item`), heal builds an
undirected graph: methods sharing a `this.foo` / `self.foo` field
reference are connected, and a sibling-method call is a direct edge.
The number of connected components is the LCOM value.

- `cluster_count == 1`: the class is cohesive.
- `cluster_count ≥ 2`: the class has separable concerns; each cluster
  could in principle become its own type.

The default `min_cluster_count = 2` filters out cohesive classes
before Severity classification; the calibrated `cluster_count`
distribution then assigns the actual tier.

**Approximation caveats** (`backend = "tree-sitter-approx"`):

- Inherited fields from a base class are invisible.
- Dynamic property access (`this[name]`) is invisible.
- A helper function shared between methods that lives outside the
  class makes the methods look unrelated.

These bias toward false positives — surfaced classes are candidates
for human review, not autonomous decisions. A typed `backend = "lsp"`
implementation lands in v0.5+.

**Languages**: TypeScript class scope, Rust impl block. Module-scope
LCOM (Rust file-level free functions, TS named-export groups) is
deferred.

## How heal uses these

Every commit, heal:

1. Runs every observer (a single `run_all` pass).
2. Surfaces every Critical / High Finding to stdout via the
   post-commit nudge.

`heal status` re-runs the analysis on demand, classifies findings by
Severity, and writes a `FindingsRecord` to `.heal/findings/latest.json`.
That cache is the TODO list the `/heal-code-patch` skill drains, one finding
per commit.

The exact JSON shapes and storage details are documented in
[Architecture › The findings cache](/heal/architecture/#the-findings-cache)
for scripting.
