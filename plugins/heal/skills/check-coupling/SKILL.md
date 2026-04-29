---
name: check-coupling
description: Identify file pairs that consistently change together (logical / co-change coupling) and explain the architectural implication. Trigger on "what files move in lockstep?", "is this entangled with X?", or hidden module-dependency hunts. Read-only — proposes separations without applying them.
---

# check-coupling

## Why this metric matters

**Logical coupling** (also called *change coupling* or *co-evolution*)
comes from the SCM-mining literature — Gall et al. (1998), D'Ambros &
Lanza, and popularised for practitioners by Adam Tornhill in
*Software Design X-Rays*. It measures how often two files are committed
together. Unlike *static* coupling (imports, function calls), logical
coupling captures dependencies that the type system cannot see:

- Two files always edited together because they encode the same
  business rule in different layers (controller + view, schema + DTO).
- Two unrelated files coupled only via a third "god" file or module.
- A leaky abstraction: changes to a "private" module always force
  changes to its callers.

Empirically (Tornhill, Yamashita & Moonen), high logical coupling
without a corresponding static dependency is a strong predictor of
defects and a signal of poor modularity.

## Reading the signal

Common patterns and what they imply:

| Pair shape | Likely cause | Action? |
|------------|--------------|---------|
| `foo.rs` ↔ `tests/foo.rs` | Test follows impl | None |
| `interface.rs` ↔ `impl.rs` (same module) | Cohesive unit | None |
| `service.rs` ↔ `unrelated_helper.rs` | Hidden dependency | Investigate |
| Many files ↔ one file | God-class / facade | Decompose the hub |
| Files in different modules | Cross-cutting concern leak | Extract shared layer |

## Procedure

1. Run `heal status --metric change-coupling --json`.
2. Read `worst.pairs` (precomputed top-N by
   `metrics.top_n_change_coupling`, exposed as `top_n`). Default 5.
3. Also note `worst.files` — files with the highest sum-of-coupling.
   A single file appearing in many pairs is more interesting than a
   single hot pair: it points at a structural hub.
4. For each suspicious pair (cross-module, no static dependency
   visible), **open both files** and look for:
   - Shared third-party dependency they both wrap → **Extract Class /
     Service Layer**.
   - One referencing types/constants from the other implicitly →
     **Move Method / Move Field**.
   - Both modifying a common file or struct → **Introduce Mediator /
     Facade**, or split that struct.
5. Propose, naming the pattern:
   - **Extract Class** (Fowler): pull the shared concept into its
     own module that both depend on.
   - **Move Method / Field**: relocate behavior to whichever side
     "owns" it.
   - **Introduce Service Layer**: put cross-cutting workflow in a
     dedicated layer rather than spreading it.
   - **Bounded Context split** (DDD): when the pair sits across
     domain seams, the modularity bug is at the architecture level.
6. Confirm with the user before declaring a pair "wrong" — coupling
   is descriptive. Test ↔ impl pairs are healthy and should not be
   refactored "away".
7. Close: "want me to sketch the extraction? v0.2+ skills will
   automate it."

## When NOT to act

- Test ↔ implementation pairs: expected.
- Closely related files in the same module (e.g. `mod.rs` ↔ submodule
  files): expected.
- Bulk commits >50 files are skipped at observation time, so a sweeping
  refactor commit will not pollute rankings — no need to filter again.
- Coupling driven by a release-train artefact (version bumps, manifest
  edits): noise; suggest excluding the path in `metrics.loc.exclude_paths`.

## Constraints

- Read freely; do not edit.
- Default to **top 3 pairs** from `worst.pairs`, even if `top_n > 3`.
  Pair lists can be long; focus on the most striking.
- v0.1 reports pairs only — clusters (3+ files moving as one) arrive
  in a later release.
