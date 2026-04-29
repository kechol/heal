---
title: Metrics
description: The six metrics HEAL collects on every commit, what each one means, and when to look at it.
---

HEAL ships with six metrics. None of them are AI-specific — they are
all "boring" code-health numbers that have been around for decades.
What is new is **using them as triggers**: HEAL collects them every
commit and feeds threshold breaches to your next Claude session.

This page explains what each metric means in plain terms. For knobs,
see [Configuration](/heal/configuration/).

## When to enable what

If you are just starting:

1. **LOC** is always on. No work.
2. Leave **Cognitive** on. CCN is off but enabling it is a one-liner.
3. **Churn** is on. Once you have ~10 commits it starts being useful.
4. Once those look right, turn on **Hotspot** — it is the most
   actionable single number.
5. **Duplication** and **Change Coupling** are diagnostic; turn them
   on when you are actually investigating, not always.

## LOC — Lines of Code

> _"What does this codebase actually consist of?"_

Counts code, comment, and blank lines per language using
[`tokei`](https://github.com/XAMPPRocky/tokei). It is the foundation:
other metrics use the language list (e.g. complexity only runs on
languages it can parse, hotspot weights commits by complexity).

A representative output:

```
Primary: Rust
  Rust         18421 code  2891 comments
  TypeScript    4920 code   612 comments
  Markdown      1830 code     0 comments
```

The "primary language" is the non-literate language with the most
code lines. Markdown / Org are deliberately skipped so a docs-heavy
repo still picks the implementation language.

LOC is always on; there is no toggle. Cost is negligible — `tokei`
caches per file.

## Complexity — CCN and Cognitive

> _"Which functions are hard to follow?"_

Two per-function numbers, computed in the same tree-sitter walk:

- **CCN** (Cyclomatic Complexity) — the McCabe count of branches.
  Every `if`, `for`, `while`, `case`, `&&`, `||`, `?` adds one.
  A 10+ CCN function is starting to be hairy; 20+ is usually a
  refactor candidate.
- **Cognitive Complexity** — Sonar's readability metric. It
  penalises **nesting depth** (each level adds more) and collapses
  chained logical operators into a single increment. It correlates
  better with the subjective _"this is hard to read"_ feeling.

Both numbers are useful: CCN is the classical branching count;
Cognitive is closer to "human cost".

**Languages**: TypeScript and Rust in v0.1. JavaScript, Python, and
others come later.

## Churn — how often a file changes

> _"What is moving?"_

For each file in the repo, how many commits in the last
`since_days` (default 90) touched it, plus the total lines added
and deleted. Walks first-parent history so merge commits are not
double-counted.

A high-churn file is not necessarily bad — `package.json` and
boilerplate update often. Churn becomes interesting when **crossed
with complexity**: see [Hotspot](#hotspot--churn--complexity).

**Caveats**: rename detection is off in v0.1, so a file renamed
mid-window appears as two entries. Bulk reformats inflate
`lines_added` and `lines_deleted`; trust the commit count when raw
line totals look wrong.

## Change Coupling — files that move together

> _"What secretly depends on what?"_

For every commit, look at the set of files it touched. For every
pair of files in that set, increment a counter. After enough
commits, the highest counters reveal hidden dependencies — the
files that _always_ change together, even though the import graph
does not connect them.

Example output:

```
init.rs  ↔  paths.rs       co-changed 11×
status.rs ↔ snapshot.rs    co-changed  9×
```

This is a _diagnostic_ metric. The honest first reaction to a
high-coupling pair is "wait, why?" — it often points at a missing
abstraction.

**Bulk-commit cap**: commits touching more than 50 files (mass
reformats, dependency bumps) are skipped entirely so they cannot
fabricate coupling between unrelated files.

## Duplication — copy-pasted blocks

> _"Where did I copy-paste from?"_

Finds long runs of identical tokens (Type-1 clones) by walking
the tree-sitter parse tree and matching token windows of size
`min_tokens` (default 50). Reformatting and whitespace changes
do not hide a clone; renaming a variable does.

A representative output:

```
92 tokens duplicated in:
  - status.rs:4123–5210
  - check.rs:2811–3902
```

Long blocks usually mean an abstraction wants to be extracted.
Short ones (close to `min_tokens`) might just be similar-shaped
boilerplate. Tune `min_tokens` per project.

**Languages**: same as complexity (TypeScript, Rust).

## Hotspot — churn × complexity

> _"Where do bugs concentrate?"_

The classic "code as a crime scene" view, popularised by Adam
Tornhill. Multiplies a file's commit count (churn) by the sum of
its functions' CCN (complexity):

```
score = (weight_complexity × ccn_sum) × (weight_churn × commits)
```

Files that score high are _both_ changed often _and_ hard to
read — historically where regressions concentrate. If you only
look at one number weekly, look at this one.

The weights default to `1.0`. Bias them if your project has, say,
heavy churn but mild complexity (set `weight_complexity` higher to
not let "many small commits" dominate).

Because the formula is multiplicative, a file with great complexity
but no recent commits scores zero — that is intentional. Hotspot is
about _active_ trouble, not historical debt.

## How HEAL uses these

Every commit, HEAL writes a `MetricsSnapshot` to
`.heal/snapshots/`. `heal status` prints a summary; `heal check`
hands the data to Claude with a focused prompt. When a threshold
trips between snapshots, the SessionStart hook surfaces it as a
nudge in your next Claude session — see
[Claude plugin](/heal/claude-plugin/) for the rules.

The actual numbers and JSON shapes are in
[Architecture › Snapshots](/heal/architecture/#snapshots) for when you
want to script against them.
