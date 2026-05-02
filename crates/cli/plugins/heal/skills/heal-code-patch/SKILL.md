---
name: heal-code-patch
description: Drain the cache produced by `heal check`, fixing one finding per commit in Severity order, until the cache is empty or the user stops. Writes code, runs tests, and commits — does NOT push or open PRs. Refuses to start on a dirty worktree. Trigger on "fix the heal findings", "drain the cache", "work through the TODO list heal produced", "/heal-code-patch".
---

# heal-code-patch

Drain the cache that `heal check` produced. One finding per commit,
in Severity order, until the cache is empty (or the user stops). This
is the **write** counterpart to `/heal-code-review` — that one proposes,
this one applies.

## Mental model

`heal check` analyzes the project and writes a `CheckRecord` to
`.heal/checks/latest.json`. Each Finding has a deterministic id —
the same problem keeps the same id across runs, so a finding that
disappears from the cache after a commit is genuinely fixed (not
re-numbered).

`fixed.jsonl` is the audit trail of "skill committed a fix". The
**next** `heal check` reconciles it: if the same `finding_id` shows up
again, the entry moves to `regressed.jsonl` and the renderer warns. So
the loop is self-correcting: a botched fix surfaces on the next round.

## Role boundary: mechanical fixes only

`/heal-code-patch` applies *mechanical* refactorings — those whose
transformation rule is deterministic, locally-scoped, and does not
require domain knowledge to apply correctly. It does **not** make
architectural decisions, choose between names, or split modules along
domain seams. The judgement layer lives in `/heal-code-review` and the
readability criteria in its `references/readability.md`.

When the next finding requires architectural judgement (which name to
pick, which boundary to draw, whether to split a hub file, whether two
contexts should merge), the loop:

1. Stops and surfaces the trade-off to the user.
2. Defers the finding to `/heal-code-review` for proposal-level discussion
   instead of attempting an in-loop fix.

The allow-list / escalate-list under "Per-metric fix patterns" below
codify which refactor patterns are mechanical and which require
escalation. When the only remaining findings need Escalate-list
patterns, end the session with a summary and recommend the user run
`/heal-code-review`.

## Pre-flight (refuse to start when these fail)

1. **Clean worktree.** Run `git status --porcelain`. If anything is
   shown, stop and tell the user to commit or stash first. You cannot
   distinguish your changes from theirs once you start editing, and a
   commit-per-finding flow assumes a clean baseline. The cache also
   carries `worktree_clean=false` in this case — `heal checks` will
   show it.
2. **Cache exists.** Run `heal check --json` and capture the
   `CheckRecord`. The default flow reads `.heal/checks/latest.json`
   directly; a missing cache is auto-populated by the same invocation.
3. **Calibration exists.** If `heal check --json` shows every finding
   as `severity: "ok"`, the project hasn't been calibrated yet — say so
   and suggest `heal init` or `heal calibrate --force`. Don't try to
   fix Ok findings; they're not actionable until thresholds are set.

## The loop

```
while there are non-Ok findings in the cache:
    pick the next one (Severity order: Critical🔥 → Critical → High🔥 → High → Medium)
    read the file(s); plan the smallest fix that addresses the metric
    apply the change
    run tests / type-check / linter (best effort, see "Verification")
    git add -p / git add <file>; git commit -m "<conventional message>"
    heal fix mark --finding-id <id> --commit-sha <new SHA>
    heal check --refresh --json   # re-scan and overwrite latest.json
    if the finding is back (regressed warning):
        leave it for now; record in session notes; continue with next finding
    else:
        continue
```

Stop conditions: cache empty, user interrupts (Ctrl+C / Stop), or you
hit a finding that genuinely needs human judgement (architectural
decision, business rule). In the last case, surface the trade-offs and
ask before applying.

## Picking the next finding

`heal check` classifies every finding into a drain tier via
`[policy.drain]`. The default loop drains **only T0** (`must`):

1. **T0 — Drain queue** (default `["critical:hotspot"]`). The loop
   targets these and only these.
2. **T1 — Should drain** (default `["critical", "high:hotspot"]`).
   Treat as advisory; surface the trade-off and ask before draining.
3. **Advisory** — anything else above Ok. Never drain in-loop.

Within T0, iterate in `Severity 🔥` order: `Critical 🔥` first, then
any other entries the user's `must` policy admits. Skip findings
already present in `.heal/checks/fixed.jsonl` (match by `finding_id`).

If the user invoked `/heal-code-patch --metric <name>`, restrict the
selection to that metric. Default = no filter.

When T0 is empty, end the session — do **not** silently extend into T1
or Advisory. Surface the remaining tiers in the summary, recommend the
user run `/heal-code-review` if they want to act on the architectural
items, and stop.

## Per-metric fix patterns

Catalogue of patterns relevant to each metric. **Not all are mechanical** —
the Allow-list / Escalate-list below decide what this loop applies vs
surfaces. Always consult both before acting.

- **`ccn` / `cognitive`** — Decompose Conditional, Extract Function,
  Replace Nested Conditional with Guard Clauses, Replace Conditional
  with Polymorphism. Mostly escalate; only Decompose Conditional with
  a genuinely deep helper is allow.
- **`duplication`** — Form Template Method, Pull Up Method, Replace
  Conditional with Lookup Table, Consolidate Duplicate Conditional
  Fragments, Extract Function. Confirm the duplication is *real*
  (same intent), not coincidental (license headers, generated code,
  boilerplate). Apply Rule of Three: extract on the third occurrence,
  not the second.
- **`change_coupling`** — Almost always escalate. Signals a boundary
  question, not a helper extraction. Surface the trade-off; do not
  guess.
- **`hotspot`** — A *flag*, not a problem. Walk the file's other
  findings and pick from those.

Read the file before making the change. The metric might be measuring
something intentional (parser tables, exhaustive `match` arms, generated
code). If the finding is a false positive, log it in session notes and
move on without committing.

### Allow-list (apply mechanically)

The following patterns from `heal-code-review/references/architecture.md`
§5 are mechanical — apply without asking, after reading the file to
confirm the pattern fits:

- **Form Template Method.** Apply when N call sites are byte-identical
  except for the varying parameter (predicate, transform, message).
  Verify identity by reading at least two sites in full.
- **Replace Conditional with Lookup Table / Map.** Apply when the
  conditional chain is a pure equality cascade (no side-effect, no
  fall-through, no early-return semantics).
- **Consolidate Duplicate Conditional Fragments.** Apply when every
  branch ends with the same statement(s).
- **Decompose Conditional.** Apply when the named helper's interface
  (its signature) is at least three times narrower than its body — the
  deep-module test passes (cf. `architecture.md` §1).
- **Extract Variable.** Apply for an intermediate computation reused 2+
  times within the same function, where naming reveals intent.
- **Replace Magic Number / String with Named Constant.** Apply when
  the value appears in multiple places and its meaning is fixed.

### Escalate-list (stop and ask the user)

These patterns require judgement that this skill should not make alone.
When the next finding's best-fit pattern is here, stop the loop, surface
the trade-off, and let the user (or `/heal-code-review`) decide:

- **Replace Conditional with Polymorphism.** Picks the dispatch axis,
  which is an architectural choice with downstream consequences.
- **Extract Class.** Picks the seam between cohesion clusters; the
  resulting names are domain-language calls.
- **Move Function / Move Field.** Changes module boundaries; affects
  imports across the codebase.
- **Substitute Algorithm.** Requires behavioural-equivalence
  confirmation that this skill cannot make safely.
- **Replace Nested Conditional with Guard Clauses.** Only safe when
  the original is genuinely deeply-nested (see "Anti-patterns to stop
  on mid-loop" below). Reflexive application damages the rule's
  visibility.
- **Anything in Tier 5** of the leverage hierarchy — Strangler Fig,
  Branch by Abstraction, Anti-Corruption Layer, Bounded Context split,
  Split Hub File, Introduce Port. Strategic moves spanning the
  codebase, always architectural.

If the only remaining findings require Escalate-list patterns, end the
session with the summary format below and recommend the user run
`/heal-code-review` to discuss the architectural moves at the proposal
level.

## Anti-patterns to stop on mid-loop

Three failure modes that compound damage if you don't stop early. Theory
in `heal-code-review/references/architecture.md` §6 — here, only the
operational signals.

- **Relocate trap.** Signal: after Extract Function, a new helper itself
  appears critical / high; global severity barely moves. Diagnosis: the
  original complexity was intrinsic (coherent pipeline / state machine /
  dispatcher). Action: stop splitting; accept the score; move to a
  different finding.

- **Reflexive guard-clause trap.** Signal: you're about to convert a flat
  `if (A && B && C)` to `if (!A) return; …`. The original is not nested,
  so Cognitive does not drop — and inverting positives into negatives
  raises reader load. Action: only flatten genuinely nested code; leave
  flat composites alone (optionally name them: `const isRisky = …`).

- **Drain-to-zero trap.** Signal: only intrinsic / cohesive findings
  remain. Action: stop. Surface the remainder as `metrics.exclude_paths`
  candidates or deferred design questions. ROI on heal-driven refactoring
  drops sharply once symptomatic findings are gone.

Surface the trade-off **before** committing further fixes, not after.

## Verification per commit

You don't know the user's test runner. Best-effort detection:

- `Cargo.toml` exists → `cargo test` (or `cargo build` if tests are
  expensive)
- `package.json` with `test` script → `npm test` / `pnpm test` /
  `yarn test`
- `pyproject.toml` with `pytest` config → `pytest`
- `go.mod` → `go test ./...`

If there's no obvious runner, fall back to the project's lint /
type-check (`tsc --noEmit`, `cargo check`, `mypy .`). If everything
fails to detect, do a syntax check: `rustc --edition 2021 --emit=metadata`
or equivalent.

If a verification step fails, **revert your change** (`git restore .`)
and skip the finding — don't commit broken code. Move to the next
finding.

## Commit message format

Conventional Commits, with the finding id as the trailing tag so it's
greppable later:

```
fix(heal): reduce CCN in src/payments/engine.ts:processOrder

Extract the input-validation block into a helper. CCN drops from
28 to 12.

Refs: F#ccn:src/payments/engine.ts:processOrder:9f8e7d6c5b4a3210
```

Subject line: `fix(heal): <metric-specific verb> in <file>:<symbol>`.
Body: 2-3 sentences on the technique used and the expected metric
movement. Trailer: `Refs: F#<finding_id>` (the full id from cache JSON).

## Marking the commit

After the commit succeeds:

```
heal fix mark \
  --finding-id "<finding_id from cache JSON>" \
  --commit-sha "$(git rev-parse HEAD)"
```

Then run `heal check --refresh --json` to re-scan (default `heal check`
just re-reads the now-stale cache). The new cache will either confirm
the finding is gone, or `heal check` itself will print a regressed
warning and move the entry to `regressed.jsonl` automatically.

## Output format

While running, narrate progress concisely — one short paragraph per
finding:

```
[1/12] 🔴 Critical 🔥  src/payments/engine.ts  CCN=28
  Extracting validateOrder() to drop the nested input checks.
  cargo test → green. Committed: a1b2c3d4. heal check confirms fixed.

[2/12] 🔴 Critical    src/legacy/old_parser.ts  CCN=31
  ...
```

When you stop (cache drained or user interrupt), end with a summary:

```
Session summary: fixed 8 / skipped 2 / regressed 1 / 1 still pending.
Next: review the commits with `git log --oneline`, then push when ready.
```

Skipped findings stay in the cache for the next session — no need to
record them anywhere persistent.

## When NOT to act

- **Architectural decisions.** A `change_coupling` finding between
  `auth/*` and `billing/*` isn't a refactor — it's a question about
  module boundaries. Surface it; don't fix it.
- **Generated code.** Parser tables, schema-derived types, snapshot
  fixtures: high CCN / duplication is the cost of the generator. Skip.
- **Domain logic with explicit invariants.** A 30-arm match that
  enforces an exhaustive enum is intentional — splitting it loses the
  type-checker's coverage guarantee.
- **Dirty worktree.** Already covered in pre-flight; restate if the
  user asks why you stopped.

## Constraints

- One finding = one commit. Don't squash multiple findings into a
  single commit even when they share a file — the audit trail matters
  for `heal fix diff`.
- **Never push.** The skill commits locally; the user runs
  `git push` / `gh pr create` themselves.
- **Never amend.** A new commit per finding is the contract — amending
  rewrites history and breaks the `mark-fixed` ↔ commit linkage.
- **Never `--no-verify`.** If pre-commit hooks fail, fix the underlying
  issue (or revert and skip).
- Don't extend the loop beyond what the cache says. New findings the
  user wants addressed go into a new `heal check` run.
