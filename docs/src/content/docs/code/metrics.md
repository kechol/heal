---
title: Code · Metrics
description: The seven code-health metrics heal collects, the Severity ladder, the drain tier model, and why Hotspot is the one to watch first.
---

heal's always-on Code family ships seven metrics. None are
AI-specific — each is a long-standing code-health metric with
decades of literature behind it. heal's contribution is **calibrating
them to the codebase's own distribution**: a 200-line script and a
200kloc service trigger differently for the same raw value.

This page covers how Severity is decided, how findings are bucketed
for action, what each metric measures, and the special role of
Hotspot. For configuration knobs see
[Code › Configuration](/heal/code/configuration/).

## Severity ladder

Every Finding gets `Critical`, `High`, `Medium`, or `Ok`,
classified in two stages.

**Stage 1 — absolute floors (literature-anchored).** The escape
hatches that keep the percentile classifier honest at the extremes:

| Rule                     | Result   | Why                                                                                             |
| ------------------------ | -------- | ----------------------------------------------------------------------------------------------- |
| `value ≥ floor_critical` | Critical | Worst cases stay Critical even on a uniformly-bad codebase (CCN 25, Cognitive 50, Dup 30%).    |
| `value < floor_ok`       | Ok       | Graduation gate (proxy metrics only). A clean codebase isn't held hostage by "top 10% is red". |

**Stage 2 — codebase's own percentile distribution.** Anything
between the two floors is classified by where it lands in the
distribution captured at calibration time:

| Rule          | Result   |
| ------------- | -------- |
| `value ≥ p95` | Critical |
| `value ≥ p90` | High     |
| `value ≥ p75` | Medium   |
| otherwise     | Ok       |

The percentile breaks live in `.heal/calibration.toml`; the floors
are config-overridable per metric.

## Drain tiers

`heal status` groups every non-Ok Finding into one of three drain
tiers driven by `[policy.drain]`:

- **T0 — Drain queue** (default `["critical:hotspot"]`). The must-fix
  list `/heal-code-patch` drains.
- **T1 — Should drain** (default `["critical", "high:hotspot"]`).
  Bandwidth-permitting; surfaced separately, not auto-drained.
- **Advisory** — anything else above Ok. Hidden unless `--all`.

The split is what makes "drain to zero" meaningful: T0 is the goal,
T1 is hygiene, Advisory is review-when-convenient. CCN as a _proxy_
metric belongs in T0 only when corroborated by hotspot — otherwise
the metric becomes a Goodhart loop.

## The metrics

Six observers run per commit; the seventh — **Hotspot** — composes
them. Configuration knobs live in
[Code › Configuration](/heal/code/configuration/).

### LOC — Lines of Code

> _"What does this codebase consist of?"_

Counts code, comment, and blank lines per language using
[`tokei`](https://github.com/XAMPPRocky/tokei). LOC is foundational:
other metrics depend on its language detection (complexity only
runs on languages it can parse, hotspot weights commits by
complexity).

The "primary language" is the non-literate language with the most
code lines. Markdown and Org are deliberately excluded so a
documentation-heavy repository still resolves to its implementation
language.

LOC is always enabled and has no toggle.

### Complexity — CCN and Cognitive

> _"Which functions are difficult to follow?"_

Two per-function metrics, computed in a single pass:

- **CCN** (Cyclomatic Complexity) — McCabe's count of branches.
  Each `if`, `for`, `while`, `case`, `&&`, `||`, `?` adds one.
- **Cognitive Complexity** — Sonar's readability metric.
  Penalises nesting depth (each level adds increasingly more) and
  collapses chained logical operators into a single increment.

Both metrics calibrate independently:

- CCN: `floor_critical = 25` (McCabe "untestable"),
  `floor_ok = 11` (McCabe "simple").
- Cognitive: `floor_critical = 50` (SonarQube Critical),
  `floor_ok = 8` (half of Sonar's "review" threshold).

Functions strictly below `floor_ok` classify as Ok regardless of
percentile — see [Why CCN and Cognitive are
proxies](#why-ccn-and-cognitive-are-proxies) for the rationale.

**Languages**: TypeScript, JavaScript, Python, Go, Scala, Rust.

### Churn — how often a file changes

> _"What is moving?"_

Per-file commit count and added/deleted line totals over the last
`since_days` window (default 90), using first-parent history so
merge commits aren't double-counted.

A high-churn file isn't inherently a problem — `package.json`
changes frequently and that's fine. Churn becomes meaningful when
crossed with complexity — see [Hotspot](#hotspot).

Churn doesn't have its own Severity ladder; it feeds Hotspot and
the post-commit nudge.

### Change Coupling — files that move together

> _"Which files depend on which, implicitly?"_

For every commit, the set of paths it touches becomes one
co-occurrence event. Per-pair counters reveal implicit dependencies
that the import graph doesn't show.

Each pair is also classified as **Symmetric** or **OneWay**:

- **Symmetric** — both files rarely change without the other. The
  strongest "responsibility mixing" signal.
- **OneWay { from, to }** — `from` changes alone often; `to`
  almost always tags along.

Symmetric pairs surface under `change_coupling.symmetric`; the
renderer separates them so they're visible as a stronger signal
than the generic counter.

**Noise filtering.** Pairs that look like coupling but aren't
(lockfile bumps, generated artifacts, `mod.rs ↔ sibling`) are
dropped or demoted to Advisory automatically. Test ↔ source and
doc ↔ source pairs are demoted to Advisory by default since
they're expected to move together. When `[features.test]` is on,
test ↔ source pairs that have **fallen behind** their source are
re-promoted to Medium under `change_coupling.drift` — see
[Test › Metrics](/heal/test/metrics/#change-coupling-drift).

**Bulk-commit cap.** Commits touching more than 50 files are
skipped entirely so mass reformats can't fabricate coupling
between unrelated files.

### Duplication — copied blocks

> _"Where are the duplicates?"_

Finds long runs of identical tokens (Type-1 clones) by walking the
parse tree and matching token windows of size `min_tokens` (default
50). Reformatting and whitespace changes don't hide a clone;
renaming a variable does.

Calibration uses the per-file duplicate-percentage distribution;
`floor_critical = 30%` (a third of the file is duplicate is a
structural problem).

When `[features.docs]` is on, a parallel pass runs over Markdown /
RST files with a separate window length. See
[Docs › Metrics](/heal/docs/metrics/#markdown-duplication).

**Languages**: same as complexity (TypeScript, JavaScript, Python,
Go, Scala, Rust).

### LCOM — Lack of Cohesion of Methods

> _"Which classes are mechanically separable?"_

Per class, heal builds a graph: methods that share a `this.foo` /
`self.foo` field reference are connected, and a sibling-method call
is a direct edge. The number of connected components is the LCOM
value.

- `cluster_count == 1` — the class is cohesive.
- `cluster_count ≥ 2` — the class has separable concerns; each
  cluster could in principle become its own type.

The default `min_cluster_count = 2` filters out cohesive classes
before Severity classification.

**Approximation caveats** (current syntactic backend):

- Inherited fields from a base class are invisible.
- Dynamic property access (`this[name]`) is invisible.
- A helper function shared between methods that lives outside the
  class makes the methods look unrelated.

These bias toward false positives — surfaced classes are candidates
for human review, not autonomous decisions. A type-aware backend is
on the v0.5+ roadmap.

**Languages**: TypeScript / JavaScript class scope, Python class
scope, Rust impl block. Go has no class scope; Scala is deferred to
the type-aware backend.

## Hotspot

Hotspot is special — it isn't a metric on its own, it's a
**leverage multiplier on top of the other metrics**.

> _"Where do regressions concentrate?"_

The "code as a crime scene" view popularised by Adam Tornhill.
Hotspot multiplies a file's commit count (churn) by the sum of its
functions' CCN (complexity):

```
score = (weight_complexity × ccn_sum) × (weight_churn × commits)
```

The output is **not a Severity tier** but a per-file flag (top-10%
of the score distribution) that the renderer surfaces as the `🔥`
emoji on top of any other finding for that file. A finding can be
`Critical 🔥`, `High 🔥`, `Medium 🔥`, or even `Ok 🔥`.

Hotspot is the **single most actionable signal** heal produces. A
complex file nobody touches is debt; a complex file the team edits
every other day is where the next bug ships from. The default
drain queue (`critical:hotspot`) exists because that intersection
is where every minute of refactor pays back the most.

The formula is multiplicative, so a file with high complexity but
no recent commits scores zero — Hotspot identifies **active**
trouble, not historical debt.

The "Ok 🔥" subset — low Severity but heavily touched, "why are we
still editing this?" candidates — appears in a dedicated section
under `heal status --all`.

## Why CCN and Cognitive are _proxies_

McCabe (1976) introduced CCN as a static estimate of the minimum
number of test cases needed for branch coverage — not as a
code-quality metric. Sonar's Cognitive Complexity (2017) is a
readability proxy. Driving either toward zero damages readability:

- Extract Function on a procedurally cohesive function relocates
  CCN rather than reducing the global count.
- Converting flat positive composites (`if (A && B && C)`) to
  negative guard chains doesn't move Cognitive (the original isn't
  nested) and often _increases_ reader load.

heal's design accepts this. `floor_ok` graduates clean codebases
off the proxy metrics. Hotspot multiplies leverage on touched
files. The drain-tier model keeps the TODO list focused on findings
where the proxy and the underlying problem agree.

## How heal uses these

Every commit:

1. The post-commit hook runs every observer once.
2. Critical / High findings are printed to stdout — the next
   problem stays visible without a daemon.
3. When `[features.test.coverage]` is on, an additional line names
   any uncovered hotspot count.

`heal status` re-runs the analysis on demand, classifies findings
by Severity, and writes the TODO list to `.heal/findings/`. That's
what `/heal-code-patch` drains, one finding per commit. Re-running
on the same commit + config + calibration is a free cache hit.
