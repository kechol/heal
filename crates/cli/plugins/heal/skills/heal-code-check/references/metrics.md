# Code-health metrics — what each signal actually means

Reference loaded by `heal-code-check` when the skill needs to
interpret a finding. Each entry follows the same shape:

- **Definition** — the operational meaning, in one sentence.
- **Literature** — where the metric comes from.
- **Thresholds** — what counts as "fine / review / refactor",
  calibrated against the per-project distribution but anchored on
  the literature's absolute values.
- **Reads as** — what a *high* score is actually telling you, beyond
  the number.
- **False positives** — patterns that score high but are not defects.

The Severity tier on every Finding (`ok < medium < high < critical`)
is **calibrated**: the score distribution is fitted per project and
clamped to literature anchors. Read the literature thresholds below
to sanity-check a Severity assignment, but trust calibration for the
day-to-day verdict.

---

## `loc` — Lines of Code

- **Definition.** Non-blank, non-comment lines per file.
- **Literature.** No canonical threshold. LOC is a *size* metric, not a
  health metric. It correlates with defect count only because larger
  files have more places for bugs to hide (Hatton, 1997).
- **Thresholds (HEAL defaults).** Files >300 LOC drift toward review;
  >800 LOC are almost always doing two jobs.
- **Reads as.** A file that grew past its single responsibility, or a
  generated artefact that should be excluded. Not actionable on its
  own — pair with CCN, churn, or coupling to know whether the size is
  *paid for* by the work it does.
- **False positives.** Generated code (parsers, schema bindings),
  fixture data, vendored deps. Configure exclusions in
  `metrics.loc.exclude_paths`.

## `ccn` — Cyclomatic Complexity (McCabe, 1976)

- **Definition.** Independent paths through a function: `decisions + 1`.
  Maps to the minimum number of test cases for branch coverage.
- **Literature.** McCabe (1976), NIST SP 500-235, SonarQube defaults.
  - 1–10: simple, low risk.
  - 11–20: moderate; review for clarity.
  - 21–50: high risk; difficult to test.
  - 51+: untestable in practice; refactor.
- **Reads as.** "How many test cases would I need to cover every
  branch?" A function with CCN 30 needs at least 30 tests for branch
  coverage, which is rarely achieved — most CCN-30 functions are
  silently under-tested.
- **False positives.** Exhaustive `match` over a closed enum (Rust),
  generated parser tables, state machines. The CCN flags it; the
  refactor would cost the type-checker's exhaustiveness guarantee.
  Skip silently.

## `cognitive` — Cognitive Complexity (Sonar, 2017)

- **Definition.** Penalises *nesting* and breaks in linear flow,
  rewards flat structure. A long flat `if / else if / else` chain
  doesn't compound; a deeply-nested one does.
- **Literature.** G. Ann Campbell, SonarSource white paper (2017).
  Threshold ~15 = review; ~25 = refactor priority.
- **Reads as.** "How hard is this to *understand* on first reading?"
  CCN says how many paths there are; Cognitive says how hard it is to
  follow. A function high in **both** is the strongest candidate for
  decomposition.
- **CCN vs Cognitive — when they disagree.** Cognitive penalises depth
  while CCN counts decisions equally regardless of nesting. Three
  forms produce three different signals:

  | Form | CCN | Cognitive |
  |---|---:|---:|
  | `if (A && B && C) { … }` (flat) | 3 | 3 |
  | `if (!A) return; if (!B) return; if (!C) return; …` (guard chain) | 3 | 3 |
  | `if (A) { if (B) { if (C) { … } } }` (nested) | 3 | **6** (1+2+3) |

  Only the third form benefits from guard-clause flattening. Converting
  the *first* form to a guard chain inverts a positive composite into a
  negative chain without reducing either metric — and often *increases*
  reader load (the reader must mentally re-negate each guard to
  reconstruct the rule). See `architecture.md` §6 for the guard-clause
  anti-pattern.
- **False positives.** Mathematical kernels with intentional control
  flow (e.g. a parser combinator). Confirm with the user before
  recommending decomposition.

## `churn` — Change Frequency

- **Definition.** Number of commits touching a file in the lookback
  window (`metrics.churn.since_days`, default 90).
- **Literature.** Tornhill, *Your Code as a Crime Scene*. Roots in
  Hassan (2009), "Predicting faults using the complexity of code
  changes."
- **Thresholds.** Relative — the top-N churners per project. Absolute
  values aren't meaningful; a 100-commit file in a 50-commit week is
  hot, in a 50,000-commit history isn't.
- **Reads as.** "Where the team has been spending its time." High
  churn alone isn't a defect — it can mean the area is under active
  feature work. Pair with complexity (→ `hotspot`) before acting.
- **False positives.** Mass-rename commits, dependency bumps, format
  sweeps. HEAL filters commits >50 files at observation time; if a
  release-train artefact still pollutes results, exclude its path.

## `change_coupling` — Logical / Co-change Coupling

- **Definition.** File pairs that ship together in commits more often
  than chance. The directional shape (A→B) is the conditional
  probability `P(B touched | A touched)`. The symmetric shape
  (`change_coupling.symmetric`) requires both directions to clear the
  threshold.
- **Literature.** Gall, Hajek, Jazayeri (1998); D'Ambros & Lanza;
  popularised for practitioners by Tornhill, *Software Design X-Rays*.
  Empirically (Yamashita & Moonen) high logical coupling without a
  static dependency predicts defects.
- **Reads as.** "Where the type system can't see the dependency."
  Two files always edited together because they encode the same rule
  in different layers (controller + view, schema + DTO), or one file
  leaking into another via a third "god" module.
- **Reading the pair shape.**
  | Pair shape | Likely cause | Action? |
  |---|---|---|
  | `foo.rs` ↔ `tests/foo.rs` | Test follows impl | None |
  | `interface.rs` ↔ `impl.rs` (same module) | Cohesive unit | None |
  | `service.rs` ↔ `unrelated_helper.rs` | Hidden dependency | Investigate |
  | Many files ↔ one file | God-class / facade | Decompose the hub |
  | Files in different modules | Cross-cutting concern leak | Extract shared layer |
- **False positives.** Test ↔ impl pairs, `mod.rs` ↔ submodule
  files, release-train artefacts (version bumps, manifest edits).

## `duplication` — Type-1 Verbatim Duplication

- **Definition.** Byte-identical token windows ≥
  `metrics.duplication.min_tokens` (default 50, SonarQube/PMD's
  standard). HEAL only ships **Type-1**: it does not detect Type-2
  (renamed identifiers), Type-3 (small edits), or Type-4 (semantically
  equivalent but textually different).
- **Literature.** DRY principle (Hunt & Thomas, *The Pragmatic
  Programmer*); clone-detection taxonomy (Bellon et al., 2007).
  Fowler's *Rule of Three*: leave duplication on the second
  occurrence; extract on the third — only then is the abstraction
  informed by enough variation to be sound.
- **Reads as.** "Knowledge expressed in N places." Bug fixes ship to
  one site; the others rot. Watch for *coincidental similarity*:
  license headers, `derive(...)` boilerplate, snapshot fixtures,
  initializer blocks. Extracting these couples things that should
  evolve separately.
- **False positives.** Test fixtures, generated parsers / schema
  types, license/copyright headers, cross-language duplicates with
  the same intent (same JSON schema in TS and Rust — can't be DRYed
  without code-gen).

## `hotspot` — Composite (churn × complexity)

- **Definition.** `score = commits × ccn_sum × weights` per file, then
  flagged when `score ≥ p90` of the per-project distribution.
- **Literature.** Tornhill, *Your Code as a Crime Scene* and
  *Software Design X-Rays*. Empirically a small fraction of files
  (often <10%) accounts for the majority of post-release defects;
  they cluster in the hotspot list.
- **Reads as.** A *flag*, not a problem in itself. The actionable
  finding is the underlying CCN / duplication / coupling on the same
  file. When walking findings, use `hotspot=true` as a leverage
  multiplier on whichever metric drove the score.
- **One-axis cases (ignore).**
  - High complexity, low churn → mature, stable code (parsers, math
    kernels). The complexity has been paid down by reading.
  - Low complexity, high churn → boilerplate or config. Edits are
    cheap; not a target.
- **False positives.** Recently-touched files near a release window
  (high churn = planned feature work, not chaos). Defer until after
  the release.

## `lcom` — Lack of Cohesion of Methods

- **Definition.** Per class / `impl` block, the number of disjoint
  method clusters. Two methods belong to the same cluster if they
  touch a shared field or one calls the other; LCOM is the resulting
  connected-component count. `cluster_count == 1` is cohesive;
  `>= 2` means the class is mechanically separable.
- **Literature.** Chidamber & Kemerer (1991); Henderson-Sellers
  refinement; Hitz & Montazeri (LCOM4). Tornhill frames LCOM as the
  *internal* companion to change coupling — coupling reveals
  inter-file split candidates, LCOM reveals intra-file ones.
- **Reads as.** "This class wants to be two classes." A `Service`
  with two LCOM clusters often has one cluster doing read paths and
  another doing write paths, or one for caching and one for the
  underlying work. Splitting along that seam tends to drop coupling
  on the surrounding modules at the same time.
- **Approximation note.** The shipped LCOM observer is a pure
  syntactic walk (no type resolution): inherited fields, dynamic
  property access (`this[name]`), and helper-mediated state-sharing
  are invisible to it. The bias is toward over-reporting — a
  cohesive class can look split. Treat LCOM findings as candidates
  for review, not verdicts.
- **False positives.** Classes that intentionally group many small
  unrelated helpers (e.g. a `Util` namespace), trait impls whose
  methods don't share state by design, generated code.

---

## Severity ladder, calibrated

`Severity` is `Ok < Medium < High < Critical`. Per-file aggregation
takes the maximum, so a file with one Critical finding and three
Medium findings reads as Critical at the file level. Calibration
fits the per-project distribution and clamps to the literature
anchors above, so the *absolute* threshold (e.g. CCN 20) and the
*project-relative* signal ("top 5% of files in this repo") line up.

The orthogonal **`hotspot` flag** lifts a finding's leverage: a
`Critical 🔥` finding (Critical AND hotspot=true) is the highest-priority
target because the cost of a fix is multiplied by how often the file
gets touched. Same Severity without the flag is still actionable, but
the leverage is lower.

When in doubt about a Severity, read the metric's literature
threshold above and judge against absolute values — calibration is
informed by, not in conflict with, the published anchors.
