# Readability principles for refactor judgement

Reference loaded by `heal-code-check` when evaluating whether a proposed
refactor genuinely improves the code beyond moving heal's metric needles.

`architecture.md` catalogues *what* refactor patterns exist and how they
affect heal scores. This reference catalogues the *why* — the human-facing
goals refactoring serves. When a heal-driven proposal conflicts with these
goals, the proposal is wrong even if the metric would improve.

---

## 1. Goal hierarchy

Refactor proposals exist in service of three goals, in priority order:

1. **Readability.** A future reader (likely you, in six months, with no
   context) understands the code with minimal effort. *Time-to-comprehend*
   is the canonical metric (Boswell & Foucher, *The Art of Readable Code*,
   2011).
2. **Maintainability.** The code can be changed safely. Boundaries hold;
   blast radius of changes is contained; mistakes are caught early.
   Coupling and cohesion drive this layer.
3. **Heal score.** The metrics are *imperfect proxies* for (1) and (2).
   When metrics improve as a consequence of (1) or (2) improving, use
   them. When they conflict — notably the *intrinsic* and *cohesive
   procedural* categories in `SKILL.md`'s triage taxonomy — trust (1)
   and (2) and propose accepting the score.

Most heal findings align all three. The few that don't are exactly the
ones flagged in `architecture.md` §6 as the relocate trap, the reflexive
guard-clause trap, the drain-to-zero trap, and data-shaped CCN.

The triage taxonomy is the *negative* filter (which findings to skip);
this reference is the *positive* criterion (what makes a proposal
worth making in the first place).

---

## 2. Readability principles

Four canonical sources, condensed to operational rules.

### 2.1 Time-to-comprehend (Boswell & Foucher, 2011)

A refactor is good if a stranger reads the *result* faster than the
*original*. The metric is irrelevant if the stranger reads slower.

- Pack information into names: `riskAdjustedScore` > `score2` > generic
  `data` / `info` / `tmp`.
- Specific over generic: `getUserById` > `get`.
- Use only abbreviations the reader knows (`HTTP`, `JSON` ok;
  `mngr`, `prdct` not).
- Keep vocabulary consistent across the codebase (`customer` xor
  `client`, not both for the same thing).
- Make conditions read as the spec states them: `if (isHighRisk)
  escalate()` > `if (!isLowRisk) return;`.

### 2.2 Deep modules (Ousterhout, 2018)

A helper is good when its interface is much narrower than its
implementation — it *hides* complexity. A helper whose interface is as
wide as its body merely *relocates* it. See `architecture.md` §1 for
the deep-module test.

- Hide implementation details (lock ordering, internal state) — leaking
  them through the contract creates change-amplification.
- Design twice. The first split is rarely the best.

### 2.3 Four rules of simple design (Beck)

In priority order: (1) passes tests; (2) reveals intent; (3) no
duplication; (4) fewest elements. Don't sacrifice (2) for (3) or (4).
heal's `duplication` drives at (3); `ccn` / `cognitive` at (2) and (4).

### 2.4 Comments augment, not substitute (Knuth; Boswell)

- Comments explaining *what* code does → fix the names instead.
- Comments explaining *why* (constraint, invariant, workaround) → preserve
  across the refactor.

---

## 3. Heuristics for judgement in heal-code-check

Five questions to ask before proposing a refactor. If any answer is "no"
or "unsure", hold the proposal as a deferred question rather than
auto-recommend.

1. **Will the result read faster than the original?**
   The time-to-comprehend test (§2.1). If unsure, present both phrasings
   and let the user choose.

2. **Does the result hide complexity behind a name, or merely relocate
   it?**
   The deep-module test (§2.2). If the new helper's interface is as wide
   as its body, the helper is shallow — the proposal damages information
   hiding. Skip.

3. **Does the result preserve spec-as-code mapping?**
   When the original `if (A && B && C)` reads as a business rule
   verbatim, and the proposed guard-clause version reads as a chain of
   negations, the proposal damages the rule's visibility. The metric is
   indifferent; the reader is not.

4. **Does the result eliminate a real seam, or invent one?**
   Splitting a coherent procedure invents a seam (helpers without
   independent meaning). Splitting a class with two cohesion clusters
   reveals one. Only the latter deepens the modules involved.

5. **Would a reader unfamiliar with the codebase prefer the result?**
   The strongest version of (1). If you cannot argue yes from the
   stranger's seat, the proposal is at best neutral. Hold it until the
   user signals they want it anyway.

---

## 4. When this reference disagrees with a metric

heal's metrics are calibrated against literature thresholds in
`metrics.md`, but neither the metric nor the calibration captures
readability directly. When this reference and the metric disagree:

| Metric | Readability | Action |
|---|---|---|
| **High** | **Good** (intrinsic complexity, named well) | Propose `metrics.exclude_paths` — false positive on this code |
| **High** | **Poor** | Propose the refactor — genuine target, both axes agree |
| **Ok** | **Poor** | Propose the refactor anyway, framed as readability not metric improvement |
| **High** | **Would worsen** with the metric-driven fix | Reject the fix; surface as a §6 trap |

The third row — *metric ok; readability poor* — is invisible to heal
but worth surfacing during exploration. heal-code-check should
occasionally recommend a refactor with no finding to back it, when
the reading suggests one is warranted.

The fourth row is the case where the skill's defence kicks in: a
metric-driven fix would damage readability. The trap categories in
`architecture.md` §6 cover the recurring cases (relocate, reflexive
guard-clause, drain-to-zero, data-shaped CCN). New traps can be
added there as they're recognised.

---

## How `heal-code-check` should use this reference

When proposing a refactor in Phase 2:

1. After identifying a candidate via the metric and the triage
   taxonomy, run the **5-question judgement test** in §3.
2. If all five answers are clearly "yes", propose the refactor as a
   TODO entry.
3. If any answer is "no" or "unsure", hold the proposal as a
   *deferred question* — present the trade-off in the deferred-questions
   section rather than auto-recommend.
4. When the metric is silent but the reading suggests a refactor (§4
   row 3), surface the suggestion explicitly framed as a readability
   improvement, not a metric improvement. Be explicit that no heal
   finding backs it.

The goal is for the user to leave each session with code that reads
faster, not just code with a smaller cache.
