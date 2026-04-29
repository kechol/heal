---
title: Metrics
description: The six metrics heal collects on every commit, what each one means, and when to use it.
---

heal ships with six metrics. None are AI-specific; each is a
long-standing code-health metric that has been in use for decades.
heal's contribution is using them as triggers: it collects the
metrics on every commit and surfaces threshold breaches to the next
Claude session.

This page summarises each metric. For configuration knobs, see
[Configuration](/heal/configuration/).

## Recommended adoption order

When starting out:

1. **LOC** is always enabled — no action required.
2. Leave **Cognitive** enabled. **CCN** is disabled by default;
   enable it when you want classical Cyclomatic numbers as well.
3. **Churn** is enabled. It becomes informative after roughly ten
   commits in the lookback window.
4. Once those look reasonable, enable **Hotspot** — the most
   actionable single metric.
5. **Duplication** and **Change Coupling** are diagnostic. Enable
   them when investigating; otherwise leave them off.

## LOC — Lines of Code

> _"What does this codebase consist of?"_

Counts code, comment, and blank lines per language using
[`tokei`](https://github.com/XAMPPRocky/tokei). LOC is foundational:
other metrics depend on its language detection (complexity only runs
on languages it can parse, hotspot weights commits by complexity).

Sample output:

```
Primary: Rust
  Rust         18421 code  2891 comments
  TypeScript    4920 code   612 comments
  Markdown      1830 code     0 comments
```

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
  Functions above 10 are typically borderline; above 20 they are
  refactor candidates.
- **Cognitive Complexity** — Sonar's readability metric. It
  penalises **nesting depth** (each level adds increasingly more)
  and collapses chained logical operators into a single increment.
  It correlates more closely with the subjective "hard to read"
  judgement.

Both metrics are useful: CCN is the classical branching count;
Cognitive is closer to a perceived-cost measure.

**Languages**: TypeScript and Rust. Additional languages arrive in
later releases.

## Churn — how often a file changes

> _"What is moving?"_

For each file in the repository, churn reports the number of commits
in the last `since_days` window (default 90) that touched it,
together with total lines added and deleted. The walk uses
first-parent history so merge commits are not double-counted.

A high-churn file is not inherently problematic — `package.json`
and similar boilerplate change frequently. Churn becomes meaningful
when crossed with complexity (see [Hotspot](#hotspot--churn--complexity)).

**Caveats**: rename detection is currently disabled, so a file
renamed mid-window appears as two entries. Bulk reformats inflate
`lines_added` and `lines_deleted`; trust the commit count when raw
line totals look misleading.

## Change Coupling — files that move together

> _"Which files depend on which, implicitly?"_

For each commit, change coupling examines the set of files it
touched. For every pair of files in that set, a counter is
incremented. After enough commits, the highest counters reveal
implicit dependencies — files that consistently change together
even though the import graph does not connect them.

Sample output:

```
init.rs  ↔  paths.rs       co-changed 11×
status.rs ↔ snapshot.rs    co-changed  9×
```

This is a diagnostic metric. A high-coupling pair often indicates
that an abstraction is missing between the two files.

**Bulk-commit cap**: commits touching more than 50 files (mass
reformats, dependency bumps) are skipped entirely so they cannot
fabricate coupling between unrelated files.

## Duplication — copied blocks

> _"Where are the duplicates?"_

Finds long runs of identical tokens (Type-1 clones) by walking the
tree-sitter parse tree and matching token windows of size
`min_tokens` (default 50). Reformatting and whitespace changes do
not hide a clone; renaming a variable does.

Sample output:

```
92 tokens duplicated in:
  - status.rs:4123–5210
  - check.rs:2811–3902
```

Long blocks usually indicate that an abstraction can be extracted.
Short blocks (near `min_tokens`) may simply be similar-shaped
boilerplate. Tune `min_tokens` per project.

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
to read — historically where regressions concentrate. If only one
metric is reviewed weekly, this is the recommended one.

The weights default to `1.0`. Adjust them for projects with
asymmetric profiles — for example, raise `weight_complexity` to
prevent "many small commits" from dominating the score.

The formula is multiplicative, so a file with high complexity but no
recent commits scores zero. This is intentional: hotspot is meant to
identify _active_ trouble, not historical debt.

## How heal uses these

Every commit, heal writes a `MetricsSnapshot` to `.heal/snapshots/`.
`heal status` prints the summary; `heal check` hands the data to
Claude with a focused prompt. When a threshold is crossed between
snapshots, the SessionStart hook surfaces it as a notice in the next
Claude session — see [Claude plugin](/heal/claude-plugin/) for the
rule list.

The exact JSON shapes and storage details are documented in
[Architecture › Snapshots](/heal/architecture/#snapshots) for
scripting.
