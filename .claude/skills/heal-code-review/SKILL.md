---
name: heal-code-review
description: Read every finding produced by `heal status --all --json`, deeply investigate the user's codebase, and return one architectural reading plus a prioritised refactor TODO list — grounded in the metric literature and module-depth / layering / DDD vocabulary. Works on any language and shape of project; respects the codebase's existing design. Read-only — proposes only. The write counterpart is `/heal-code-patch`. Trigger on "what does heal say?", "review the codebase health", "where should we refactor?", "/heal-code-review".
metadata:
  heal-version: 0.2.1
  heal-source: bundled
---

# heal-code-review

One entry point for *understanding* what `heal status` has found.
Runs across every metric in a single pass and returns one ranked
TODO list with architecture-level reasoning, instead of fragmenting
the same data into per-metric views.

The output is two artefacts:

1. An **architectural reading** of the codebase — what the findings
   tell you, taken as a system rather than a list.
2. A **prioritised TODO list** — concrete refactors keyed to specific
   files / functions, each tagged with the established refactor
   pattern and its expected metric movement.

This skill *proposes*; it does not edit code. The write counterpart
is `/heal-code-patch`, which drains the same cache one finding per
commit.

## Audience

The skill applies to **any** project that has run `heal init` and
recorded a `FindingsRecord`. It is language-agnostic — observers ship
for the languages `heal` supports, and the skill consumes their
output without further parsing. Refactor proposals are tailored to
the codebase's apparent style, not pushed from a one-size-fits-all
template (see `references/architecture.md` §4).

## References (load on demand)

Three reference files live next to this `SKILL.md`. Load them when
the skill body says "see references/…" — they're kept out of the
main prompt so it stays terse.

- `references/metrics.md` — what each metric (`loc`, `ccn`,
  `cognitive`, `churn`, `change_coupling`, `duplication`, `hotspot`,
  `lcom`) measures, the literature behind it, the thresholds, and
  the typical false positives.
- `references/architecture.md` — the vocabulary used for refactor
  proposals: module depth (Ousterhout), layered / hexagonal
  architecture (Cockburn, Evans), DDD (Evans, Vernon), the leverage
  hierarchy of refactor patterns, the trap catalogue, plus the
  rules for *respecting the codebase* the proposals must pass.
- `references/readability.md` — the *positive* criterion for
  proposals: the goal hierarchy (readability → maintainability →
  metric), readability principles (Boswell, Ousterhout, Beck,
  Knuth), and the 5-question judgement test for whether a refactor
  proposal is worth making.

## Mental model

`heal status --all --json` emits a `FindingsRecord` containing every
classified `Finding`:

```jsonc
{
  "version": 2,
  "id": "...",                // ULID; lexicographic order = chronological
  "head_sha": "...",
  "worktree_clean": true,
  "severity_counts": { "critical": 3, "high": 11, "medium": 22, "ok": 0 },
  "findings": [
    {
      "id": "ccn:src/payments/engine.ts:processOrder:9f8e7d6c5b4a3210",
      "metric": "ccn",
      "severity": "critical",
      "hotspot": true,
      "location":  { "file": "src/payments/engine.ts", "line": 120, "symbol": "processOrder" },
      "locations": [],         // multi-site findings (duplication / coupling) populate this
      "summary":   "CCN=28",
      "fix_hint":  "Extract input validation"
    },
    ...
  ]
}
```

Each `Finding.id` is decision-stable: the same problem keeps the
same id across runs. The cache is therefore a TODO list — the
skill's job is to read it as a *system* (not walk it linearly) and
surface the dominant signal, the highest-leverage moves, and the
findings that are architectural questions rather than refactors.

## Goal hierarchy

Metrics are proxies. Priority order for any proposal:

1. **Readability** — time-to-comprehend for a future reader.
2. **Maintainability** — boundaries, coupling, blast radius.
3. **Heal score** — improves because (1) or (2) improved, never as the
   goal itself.

When the metric and (1)/(2) disagree (the *intrinsic* and *cohesive
procedural* cases in the triage taxonomy below), trust readability and
maintainability. See `references/readability.md` §1 for the full
hierarchy and §3 for the 5-question judgement test that every Phase 2
proposal must pass.

## Pre-flight

Stop and ask before proceeding if any of these are off:

1. **Cache exists.** Run `heal status --all --json`. If the cache is
   missing or stale, the same invocation refreshes it. Capture the
   full payload.
2. **Calibrated.** If every finding has `severity: "ok"`, the
   project hasn't been calibrated. Tell the user to run
   `heal init` (first time) or `heal calibrate --force`
   (re-baseline). Don't proceed — the ranking would be meaningless.
3. **Worktree state noted.** If `worktree_clean` is false, mention
   it once in the architectural reading: numbers reflect committed
   state plus uncommitted drift.

## Procedure (Explore → Synthesise → Grill)

Three phases: explore the system, present candidates, then walk
the design tree with the user.

### Phase 1 — Explore

1. **Capture the cache.** Read the full `FindingsRecord` JSON.
2. **Cluster the findings.**
   - **By file.** Multiple findings on one path → architectural
     target.
   - **By metric.** Which signal dominates — does this codebase
     have a complexity problem, a duplication problem, a coupling
     problem? The dominant axis sets the reading's frame.
   - **By hotspot flag.** `hotspot=true` is a leverage multiplier;
     the same Severity with the flag should usually outrank without.
3. **Read the top files.** For every file with `≥ 2` non-Ok
   findings, *or* a Critical finding, *or* `hotspot=true`: open
   the file. Summarise what it does in one sentence. Don't trust
   the metric summary alone — the score might be measuring
   something intentional (parser tables, exhaustive enums,
   generated code, fixture data).
4. **Look for cross-cutting patterns.**
   - `change_coupling` pairs spanning different module roots →
     candidate hidden seam.
   - A single file showing up in many coupling pairs → structural
     hub.
   - `regressed.jsonl` entries (re-detected after a recorded fix)
     → the previous fix addressed the symptom, not the cause.
     Treat as priority questions.
5. **Infer the codebase's existing design** before proposing.
   Skim the directory tree once, in your own pass:
   - Languages and tooling present.
   - Layering convention (flat `src/`, `domain/application/infra`,
     `controller/service/repository`, by-feature, by-type).
   - Style cues (functional vs OO, explicit DI vs ambient,
     composition vs inheritance, sync vs async).
   The proposals in Phase 2 must fit this grammar — see
   `references/architecture.md` §4.

For interpretation of any specific metric, **read
`references/metrics.md`**. Don't paraphrase from memory — the
thresholds and false-positive lists are the authoritative version.

### Phase 2 — Synthesise

Produce two things, in order.

#### Architectural reading (3–6 lines)

Say *as a system* what the cache is telling you. Examples of the
shape:

- "Complexity is concentrated in two files (`payments/engine.ts`,
  `inventory/sync.ts`); both are Critical CCN with `hotspot=true`.
  The change-coupling layer is quiet, suggesting these modules are
  *internally* tangled rather than *between* tangled."
- "The dominant signal is duplication across the `commands/` tree.
  Three command files share a near-identical render block — this
  is a Pull Up Method candidate, not a per-command fix."
- "One hub: `core/store.ts` couples to seven other files. It has
  accreted both 'persistence' and 'business workflow'
  responsibilities. The leverage move is to split along that seam,
  not to fix any individual coupling pair."

The reading **must** name the dominant axis. If you can't, say so —
"the findings don't cluster cleanly; there's no single architectural
theme" is a valid output, and tells the user to attack the
highest-Severity items individually.

#### Prioritised TODO list

`heal status` groups findings into three drain tiers driven by
`[policy.drain]`:

- **T0 — Drain queue** (default `["critical:hotspot"]`). The TODO list
  is **only T0** by default. These are the must-fix items.
- **T1 — Should drain** (default `["critical", "high:hotspot"]`).
  Surface as a separate "If bandwidth permits" section after the TODO,
  not as TODO entries.
- **Advisory** — everything else above `Severity::Ok`. Mention as a
  count, never as TODO entries.

Within T0, sort `Critical 🔥` first. Cap the TODO list at the top 8 —
beyond that the list dilutes. If the user asked for "everything", you
may extend into T1; never auto-extend into Advisory.

Each entry is exactly **5 lines**:

```
[N] <severity><🔥 if hotspot>  <metric short_label>  <file>:<symbol-or-line>
    What this code does:    <one sentence after reading it>
    Why it scores:          <root cause: nesting / mixed responsibilities / hidden seam / hub / etc>
    Proposed move:          <named pattern from references/architecture.md, with target>
    Expected drop:          <which metric(s) move, by roughly how much; or "verifies on next heal status">
    finding-id:             <id from FindingsRecord — exact, so heal-code-patch can pick it up>
```

Reach for the smallest vocabulary layer that fits the finding (see
`references/architecture.md`):

- **Module-level** (Extract Function, Replace Nested Conditional
  with Guard Clauses, Decompose Conditional, Replace Conditional
  with Polymorphism, Pull Up Method, Form Template Method,
  Introduce Parameter Object) — for per-file findings.
- **Layered / hexagonal** (Introduce Port, push trait/interface
  into the inner layer, Anti-Corruption Layer, Strangler Fig) —
  for cross-module `change_coupling` findings.
- **DDD strategic** (Bounded Context split, Aggregate boundary
  redraw, ubiquitous-language rename) — *only* when the finding
  genuinely crosses a domain seam. Most don't. Surface these as
  questions in Phase 3, not as auto-recommendations.

After the TODO list, list any **deferred questions** — findings
that look architectural but the answer needs a human call (a
coupling between two modules where either boundary is defensible).
Frame them as questions, not statements.

### Phase 3 — Grilling loop

Offer to walk the design tree with the user. The user picks one
candidate; the skill stress-tests the proposal before recommending
the move:

1. **Apply the shallow-module test.** "Would this extraction
   concentrate complexity at the call sites (good — the helper
   hides something) or just relocate the same code (the helper
   would be shallow)?" If shallow, drop the proposal.
2. **Walk dependency direction.** For layered proposals, confirm
   the direction matches the dependency rule (domain ← application
   ← infrastructure). A "fix" that adds an inward dependency is
   rejected.
3. **Check `references/architecture.md` §4** — the codebase-respect
   contract: read first, match style, match layering, no
   speculative future-proofing, no defensive checks against
   internal callers, no compatibility shims, Rule of Three on
   duplication, generated code excluded not refactored.
4. **Name the next concrete step.** "If you accept this,
   `/heal-code-patch` can drain finding `<id>` next. For
   architecture-level moves you'll write the change yourself, then
   `heal status --refresh` to confirm the scores moved."

Stop when the user has decided which moves to act on. Don't slide
into "and now I'll do all of them" — the next step is either
`/heal-code-patch` (per-finding mechanical refactors) or a human-led
architectural change.

## Triage: classify before fixing

Most bad refactors come from misclassifying a finding. Place each candidate
in one of three buckets before proposing.

| Category | Trigger examples | Verdict |
|---|---|---|
| **Symptomatic** | duplicated logic across N sites; mixed-responsibility class; `change_coupling` between layers | **Fix.** Pick a pattern from `references/architecture.md` §5. |
| **Intrinsic** | graph traversal; statistical aggregation; exhaustive `match` over a closed enum; data-shaped `??` / `&&` chains | **Skip.** Refactor relocates or destroys meaning. Propose `metrics.exclude_paths`. |
| **Cohesive procedural** | event handler with sequential phases; emit pipeline; orchestrator with N coherent steps | **Accept score.** Extract Function relocates here. Surface as a deferred question if the user insists. |

Diagnostic for misclassification: after Extract Function on what you took
to be Symptomatic, the new helper itself appears Critical / High in the
next cache. The original was Intrinsic or Cohesive — stop splitting.

When ranking Symptomatic candidates against each other, follow the
leverage hierarchy in `architecture.md` §5: duplication-driven patterns
(Form Template Method, Pull Up Method) cascade; Extract Function alone
barely moves the global score.

## When NOT to act on a finding

Per-metric false-positive lists live in `references/metrics.md`. The
Triage section above is the system-level frame; the items below are
the categorical skips that fall under *Intrinsic* or *Cohesive*:

- **Generated code.** Parser tables, AST visitors, schema-derived
  types, vendored deps. High CCN / duplication is the generator's
  cost, not a defect. Propose adding the path to the relevant
  exclude list (e.g. `metrics.loc.exclude_paths` in
  `.heal/config.toml`) instead of refactoring.
- **Test fixtures.** Snapshot, golden, mock data. Repetitive on
  purpose; readability comes from sameness.
- **Exhaustive `match` / `switch` over a closed enum.** CCN flags
  it; the refactor would lose the type-checker's exhaustiveness
  check.
- **Test ↔ implementation coupling pairs.** Expected and healthy.
- **Pre-release churn.** A file at the top of `churn` because a
  feature is shipping next week is *not* a chaos signal. Defer.
- **Release-train artefacts.** Version bumps, manifest edits,
  format sweeps drive coupling and churn that aren't structural.
  Suggest excluding the path; don't refactor.

True positives where the fix is a human call (architectural
boundary, business rule, public API contract) go to the *deferred
questions* list at the end of Phase 2 — not the TODO list.

## Output format

Cap total output at **~40 lines** for the default cache.

```
Architectural reading
  <3–6 lines>

TODO  (T0 / Drain queue — top N of M)
  [1] <5-line entry>
  [2] ...
  ...

If bandwidth permits  (T1 / Should drain — only if any)
  - <one-line summary per finding, no 5-line block>

Advisory  (count only)
  + N findings below T1 — review when convenient

Deferred questions  (only if any)
  - <one-sentence framing>:  <files>  <why it's a call, not a fix>

Next step
  Run `claude /heal-code-patch` to drain T0 one commit at a time. T1 /
  Advisory items are not part of the loop — pick one explicitly if you
  want to act on it.
```

Numbers belong in the TODO entries, not in the architectural
reading. The reading leads with intent; numbers support it.

## When the project is brand-new

- `severity_counts` empty + `worktree_clean: true` → either freshly
  initialised or genuinely in good shape. Say so plainly. If
  `.heal/findings/latest.json` is missing, suggest running
  `heal status --refresh` to materialise it.
- Every finding `severity: "ok"` → not calibrated. Stop and tell
  the user to run `heal init` or `heal calibrate --force`.

## Constraints

- **Read freely; do not edit.** The skill is read-only at the file
  level; you may open any flagged file to ground your explanation.
- **One contract — `heal status --all --json`.** Don't shell out to
  other observers, don't reimplement a metric. The cache is the
  source of truth.
- **Keep `finding-id:` lines exact.** `/heal-code-patch` reads them
  directly to pick up where the analysis left off.
- **Default top 8.** Expand only on user request — over-listing
  dilutes the signal.
- **English output by default.** The user can ask for translation
  if they prefer another language.
