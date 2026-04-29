---
name: check-hotspots
description: Identify the highest-leverage files (high churn × high complexity) and propose concrete refactors grounded in established practice. Trigger on "where should I focus refactoring?", "what are the riskiest files?", "I'm unfamiliar with this codebase, where to start?". Read-only at the file level — proposes fixes without applying them.
---

# check-hotspots

## Why this metric matters

The **hotspot** signal — popularised by Adam Tornhill (*Your Code as a
Crime Scene*, *Software Design X-Rays*) — composes **churn** (how often
a file changes) with **complexity** (how hard it is to reason about).
Files high on both are where bugs accumulate: every new change happens
in cognitively expensive code, on a path that already attracts edits.
Empirically, a small fraction of files (often <10%) accounts for the
majority of post-release defects, and they cluster in the hotspot list.

A file high in **only one** axis is usually fine:
- High complexity, low churn → mature, stable code (parsers, math kernels)
- Low complexity, high churn → boilerplate or config (rarely a target)

## Procedure

1. Run `heal status --metric hotspot --json`.
2. Read `worst.entries` (precomputed top-N by `metrics.top_n_hotspot`
   from `.heal/config.toml`, exposed as `top_n` in the JSON). Default
   to top **3** even when `top_n` is larger — too many candidates
   drown the signal for an unfamiliar reader. Expand on request.
3. For each entry:
   - **Read** the file. Summarize what it does in one sentence.
   - Identify the **dominant complexity locus** (one function, one
     class, one repeated pattern). Hotspot scores are file-level, but
     the cure is almost always at sub-file granularity.
   - Diagnose **why** it became a hotspot. Common patterns:
     - **God file / kitchen sink**: unrelated responsibilities accreted
     - **Cross-cutting concern leak**: logging, error handling,
       authorization scattered through business logic
     - **Tightly coupled facade**: the file everyone touches because
       it sits on a heavily-used path
     - **Untamed conditional**: a single function with a giant `match`
       / `switch` that grows with every feature
4. Propose a **concrete first step** per entry, naming the refactor
   pattern (Fowler's *Refactoring* vocabulary). Examples:
   - **Extract Function**: pull a coherent block (input validation,
     formatting, etc.) into a named helper. Lowest-risk move; usually
     drops both CCN and file size meaningfully.
   - **Strangler Fig**: when the file is too entangled to refactor in
     place, introduce a new module alongside and migrate call sites
     incrementally. Right move for legacy hotspots with high blast
     radius.
   - **Parallel Change** (expand-migrate-contract): for hotspots on
     hot code paths where in-place refactor risks regression.
   - **Replace Conditional with Polymorphism / Strategy**: when the
     dominant complexity is a type-discriminating switch.
   - **Move Method / Extract Class**: when the file mixes two
     responsibilities — split along the seam.
5. Cross-reference `delta.hotspot.top_files_added` for files that just
   entered the top-N. Fresh entries are the strongest call to act —
   the rank change itself is a signal that something accelerated.
6. Close with: "want me to draft any of these as a diff? `run-code-*`
   skills will automate this in v0.2."

## When NOT to act

- Generated code (parsers, schema-derived types, vendored deps): high
  CCN there is benign. Skip silently.
- Test fixtures: long, repetitive, but exist to be readable. Not a
  hotspot worth refactoring.
- Recently-touched files near a release: defer refactor until after
  the release; high churn might be planned feature work, not chaos.

## Constraints

- Read files freely; do **not** edit them.
- Default to top **3** entries; expand to top-N only on request — too
  many findings drown the signal for an unfamiliar reader.
- Lead with "what this file does" before quoting numbers. Numbers
  alone don't justify action; intent does.
