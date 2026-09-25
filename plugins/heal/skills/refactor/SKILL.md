---
name: refactor
description: Read everything heal found in the code, investigate the codebase, and propose refactors — including structural ones (split a file along its concepts, move a function to its home, extract a class, introduce a port) when heal's metrics and `[features.semantic]` verdicts agree on the seam — then apply the proposals the user approves, one commit per proposal, with tests and verification. Language-agnostic; follows the codebase's existing design. Never pushes or opens a PR. Trigger on "what does heal say?", "review the codebase health", "where should we refactor?", "fix the heal findings", "drain the heal queue", "refactor this hotspot", "/heal:refactor".
argument-hint: "[path | finding-id | plan]"
---

# /heal:refactor

One skill for understanding and fixing what heal found in the code.
It reads the findings as a system, proposes the changes that remove the
most friction, and — after the user picks — applies them one commit at a
time.

Load before starting:

- `${CLAUDE_PLUGIN_ROOT}/references/cli.md` — CLI contract, output
  language.
- `${CLAUDE_PLUGIN_ROOT}/references/apply-loop.md` — proposal format,
  approval, the per-commit loop, accepts, verification, and the report.

Load on demand, when the step below says so:

- `references/metrics.md` — what each metric measures, its thresholds,
  and its known false positives. Read it instead of paraphrasing from
  memory.
- `references/architecture.md` — module depth, layering, DDD, the
  refactor-pattern catalog ranked by leverage (§5), the traps (§6), and
  the rules for respecting the codebase (§4).
- `references/readability.md` — the goal hierarchy and the five-question
  test every proposal must pass (§3).

Arguments: a path narrows the work to findings under it
(`heal status --path`); a finding id narrows it to the proposal that
resolves that finding; `plan` stops after proposing.

## What counts as progress

1. **Readability** — a future reader understands the code faster.
2. **Maintainability** — clearer boundaries, less coupling, smaller
   blast radius per change.
3. **heal's numbers** — they fall because (1) or (2) improved, never as
   the goal. CCN, Cognitive, and duplication are proxies.

The work queue is Critical findings on hotspot files first (T0), because
that is code that is both hard to change and changing often.

## 1. Diagnose

1. **Read the findings.** `heal status --all --feature code --json`
   (plus `--path` when narrowed). Stop and suggest `/heal:setup` when
   every finding is `ok` (not calibrated) or the command fails. Set
   aside findings with `accepted: true`; mention `accepted_rereview`
   entries (accepted findings that got worse) as notes only.
2. **Take the queue.** T0 (`drain_tier: "must"`) in ascending
   `drain_rank`, then T1 (`should`). Advisory findings are context, not
   targets — use them only when they sit on a file a proposal already
   touches. Never re-derive the order or mix ranks across families.
3. **Read the code.** Open every file with two or more findings, a
   Critical finding, or `hotspot: true`, and write one sentence on what
   it does. The metric may be measuring something intentional — a
   parser table, an exhaustive `match`, generated code.
4. **Look across files.** `change_coupling` pairs that cross module
   roots (a hidden seam); a file in many pairs (a hub);
   `.heal/findings/regressed.jsonl` entries (an earlier fix treated the
   symptom).
5. **Learn the design** before proposing: languages, layering
   convention (flat, `domain/application/infra`, by feature), style
   (functional or OO, explicit or ambient dependencies). Proposals must
   fit it (`references/architecture.md` §4).
6. **Triage each candidate** — most bad refactors come from
   misclassifying the finding:

   | Class | Looks like | Then |
   |---|---|---|
   | **Symptomatic** | logic duplicated across sites; a class with mixed responsibilities; coupling across layers | propose a change |
   | **Intrinsic** | graph traversal; statistics; exhaustive `match` over a closed enum; data-shaped `&&` / `??` chains | propose accept, or `metrics.exclude_paths` for whole generated / vendored trees |
   | **Cohesive procedural** | a handler with sequential phases; an orchestrator of coherent steps | propose accept; splitting only relocates |

### With `[features.semantic]`

Findings may carry a `semantic` map and the cache may hold semantic
findings. Read every note by its confidence: at `≥ 0.9` act on it, at
`0.5–0.9` confirm it in the code first, below `0.5` ignore it. Notes
are evidence, never the decision.

- `concept_mix` (a file carries several concepts; `fix_hint` lists the
  functions per concept), `concept_misplaced` (a function whose concept
  lives in another file), `concept_scatter` (a concept with no home) —
  these turn metrics into design statements.
- `term_drift` (two words for one thing) and `name_mismatch` (the name
  promises what the body does not do).
- `semantic.triage_class` pre-classifies High / Critical CCN,
  Cognitive, and LCOM findings for the table above.
- `semantic.friction.change` / `.test` / `.read` say which friction
  dominates; let it pick the remedy (hard to test → separate logic from
  dependencies; hard to read → rename and name the steps; hard to change
  → reduce what a change must know).
- `semantic.consequence` ranks critical and user-facing code first
  among equals; `semantic.fix_ratio` says bug fixes keep landing here.
- `semantic.duplication_real: false` means the copies change for
  different reasons — do not merge them.
- `semantic.fix_pattern` and `semantic.split_points` suggest the pattern
  and the step boundaries for CCN / Cognitive.
- `semantic.gate` and `semantic.effort` are a second opinion on how big
  the change is.
- **Upcoming work.** When the user describes the next task, write it to
  a file and run `heal semantic ask --task focus --focus <file>` then
  `heal status --focus <file> --json`; files that work will touch rise
  first, and refactoring them first makes the change easier.
- No `.heal/concepts.toml` yet → suggest `/heal:setup semantic`.

## 2. Propose

Start with an **architectural reading** of three to six lines: what the
findings say as a system, naming the dominant axis (complexity,
duplication, coupling, mixed concepts). "The findings don't cluster; no
single theme" is a valid reading.

Then up to eight numbered **proposals** in the format of
`apply-loop.md` ("Proposals"), each with a named **Pattern** from
`references/architecture.md` §5 and the **Evidence** behind it. Order
them by the queue — the lowest `drain_rank` among the findings a
proposal resolves — and, among equals, by leverage (§5): patterns that
remove duplication or split a mixed module beat a lone Extract Function.

### Structural proposals

Structural changes are ordinary proposals — not deferred questions —
when **at least two independent signals agree on the same seam**:

| Signals that agree | Proposal |
|---|---|
| `concept_mix` groups match the LCOM clusters of the same type or file | split along the concepts (Extract Class / Move Function) |
| `concept_misplaced` + `change_coupling` with the target file | Move Function to the file named in `fix_hint` |
| `concept_scatter` + duplication or coupling among the scattered files | consolidate the concept into one home module |
| a file in many `change_coupling` pairs + `concept_mix` | Split Hub File along its concepts |
| `change_coupling` across layers + `friction.test` | Introduce Port; point the dependency inward |
| duplication at three or more sites with `duplication_real` not `false` | Form Template Method / Pull Up Method |

Without `[features.semantic]`, the same rule applies to what the
metrics show: LCOM clusters that line up with a coupling seam, or a hub
visible in several coupling pairs, are two signals.

A structural change backed by one signal stays a deferred question.
Mark a proposal **needs a decision** when it renames public API
(exported items, CLI flags, JSON fields, config keys), picks between two
defensible module boundaries, or makes a DDD strategic move (bounded
context split, aggregate redraw).

### Check before presenting

Every proposal passes the five-question test in
`references/readability.md` §3. With `[features.semantic]`, write the
proposals to a JSON file —
`{"proposals":[{"id":"1","text":"<the proposal>","files":["<path>"]}]}`
— and run:

```sh
heal semantic ask --task verify_proposal --focus <file> --json
```

A proposal whose `pass` is `false` moves to the deferred questions.
Exit code 2 means answer the five questions yourself.

### Presenting

```
Reading
  Complexity sits in two files; both are Critical and hotspots. Coupling is
  quiet, so the tangle is inside these modules, not between them.

Proposals  (T0: 5 shown of 7 · T1: 12 · advisory: 40)
  [1] Split src/payments/engine.ts into pricing and validation   risk: medium
      Why:       every pricing change re-reads forty lines of input checks
      Pattern:   Extract Class (architecture.md §5)
      Evidence:  concept_mix (pricing, validation) · LCOM 2 clusters · CCN 28
      Resolves:  concept_mix:src/payments/engine.ts:*:…  lcom:…  ccn:…
      Touches:   src/payments/engine.ts, src/payments/validate.ts (new), tests/payments_test.rs
  [2] …

Accept instead
  - ccn  src/parser/table.rs:dispatch — exhaustive_enum_dispatch

Deferred questions
  - Should `auth/session` or `billing/account` own plan limits?  (coupled 14×; either side is defensible)
```

Keep finding ids exact. Numbers support the proposals; the reading leads
with intent.

With the `plan` argument, or when the user only asked for a review,
stop here and say that `/heal:refactor` applies them when re-run.

## 3–5. Choose, apply, report

Follow `apply-loop.md`. Code-specific rules on top:

- **Renames.** For a non-public symbol, write two to four candidate
  names and compare them with
  `heal semantic ask --task name_choice --focus <file> --json`; rename
  only when the result says `"rename": true` (without the family, pick
  the name the glossary and nearby code already use). Public renames
  are always **needs a decision**.
- **Splitting a long function.** Cut along `semantic.split_points` or
  the function's own phases — one step per function, each named for
  its purpose — rather than pulling out branches.
- **Stop and tell the user** when you see a trap from
  `references/architecture.md` §6:
  - *Relocate* — after an extraction the new helper is itself High or
    Critical; the original was intrinsic.
  - *Reflexive guard clauses* — you are about to turn a flat
    `if (a && b && c)` into negated early returns; flatten only truly
    nested code.
  - *Drain to zero* — only intrinsic or cohesive findings remain;
    propose accepts or `metrics.exclude_paths` and stop.
- **Scope.** This skill proposes from code findings. Test and docs
  findings belong to `/heal:tests` and `/heal:docs`, but a code change
  updates the tests and docs it would otherwise break, in the same
  commit.

End the report with the next step: another `/heal:refactor` round for
the remaining T0, `/heal:tests` or `/heal:docs` when their families
have work, or a human-led change for the deferred questions.
