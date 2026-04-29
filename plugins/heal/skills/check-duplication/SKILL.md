---
name: check-duplication
description: Find duplicate code blocks and propose specific extractions grounded in established refactoring practice. Trigger on "any copy-paste?", "is this duplicated elsewhere?", "consolidate similar code". Read-only — proposes fixes without applying them.
---

# check-duplication

## Why this metric matters

The DRY principle (Hunt & Thomas, *The Pragmatic Programmer*) frames
duplication as **knowledge duplication** — the same intent expressed
in multiple places forces every change to land in N spots and silently
permits drift. Bug fixes ship to one location, the others rot.

But not all repetition is duplication:

- **Real duplication** — the same algorithm/decision/business rule
  expressed twice. Extraction reduces maintenance burden.
- **Coincidental similarity** — independent code that happens to look
  alike (initializers, license headers, similar boilerplate). Extracting
  couples things that should evolve separately.

The *Rule of Three* (Refactoring, Fowler): leave duplication alone
on the second occurrence; extract on the third — by then the abstraction
is informed by enough variation to be sound.

HEAL detects **Type-1 duplicates** (byte-identical blocks ≥ 50 tokens
by default — SonarQube/PMD's standard threshold). It does not detect:
- Type-2 (identical structure, renamed identifiers)
- Type-3 (small edits between copies)
- Type-4 (semantically equivalent but textually different)

## Procedure

1. Run `heal status --metric duplication --json`.
2. Read `worst.blocks` (precomputed top-N by `metrics.top_n_duplication`,
   exposed as `top_n` in the JSON). Default 5; cap at top 3 unless the
   user asks for more.
3. For each block, **open every location** and:
   - Confirm it's true knowledge duplication, not a coincidence.
     Generated tables, snapshot fixtures, license/copyright headers,
     `derive(...)` boilerplate are usually false positives.
   - Identify the **invariant**: what's the same intent? What name
     would describe the helper?
   - Identify the **delta** in the surrounding code: what parameters
     would the extraction take?
4. Propose a specific refactor, naming the pattern (Fowler vocabulary):
   - **Extract Function / Method**: most common. Both call sites
     become a one-line call.
   - **Pull Up Method**: when duplicates live in a class hierarchy.
   - **Form Template Method**: when the duplication has small variant
     steps inside a larger fixed sequence.
   - **Introduce Parameter Object**: when the call would take many
     arguments, group them.
   - **Replace Magic Number / Literal with Constant**: for small but
     repeated constants.
5. Quantify the gain: "extracting saves N duplicated lines and
   collapses M independent maintenance points into one."
6. If `delta.duplication.duplicate_blocks > 0`, this commit added
   duplication — flag the new block(s) first.
7. Close: "want me to draft the helper signature and call-site diffs?
   `run-code-dedupe` (v0.2) will automate this."

## When NOT to act

- Test fixtures (snapshot, golden, mock data): repetitive on purpose.
- Generated parsers, AST transformers, schema-derived types: extracting
  fights the generator.
- Code on the second occurrence (Rule of Three): wait. Premature
  abstraction is more expensive than duplication for a third edit.
- Cross-language duplicates with the same intent (e.g. same JSON
  schema in TS and Rust): can't be DRYed without code-gen.

## Constraints

- Read freely; do not edit.
- Default to top **3** of `worst.blocks` even when `top_n > 3` — too
  many candidates dilute focus for an unfamiliar reader. Expand on
  request.
- v0.1 is Type-1 only. State this when relevant ("nothing here that's
  textually identical, but renamed-identifier duplicates wouldn't show
  up in v0.1 — let me know if you want me to look for those manually").
