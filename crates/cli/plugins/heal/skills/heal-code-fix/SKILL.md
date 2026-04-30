---
name: heal-code-fix
description: Drain the cache produced by `heal check`, fixing one finding per commit in Severity order, until the cache is empty or the user stops. Writes code, runs tests, and commits — does NOT push or open PRs. Refuses to start on a dirty worktree. Trigger on "fix the heal findings", "drain the cache", "work through the TODO list heal produced", "/heal-code-fix".
---

# heal-code-fix

Drain the cache that `heal check` produced. One finding per commit,
in Severity order, until the cache is empty (or the user stops). This
is the **write** counterpart to `heal-code-check` — that one proposes,
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

Read the cache JSON; iterate in this order:

1. `severity == "critical"` AND `hotspot == true`  ← biggest leverage
2. `severity == "critical"` AND `hotspot == false`
3. `severity == "high"` AND `hotspot == true`
4. `severity == "high"` AND `hotspot == false`
5. `severity == "medium"`  ← only if the user passed `--all` or asked for "everything"

Skip findings already present in `.heal/checks/fixed.jsonl` (the next
`heal check` would have moved them out, but a session in progress
might still have stale entries — match by `finding_id`).

If the user invoked `/heal-code-fix --metric <name>`, restrict the
selection to that metric. Default = no filter.

## Per-metric fix patterns

Map metric → established refactoring:

- **`ccn` / `cognitive`** — Extract Function (Fowler), Replace Nested
  Conditional with Guard Clauses, Decompose Conditional, Replace
  Conditional with Polymorphism. Pull out a coherent sub-block first;
  re-run `heal check` and see the number drop.
- **`duplication`** — Extract Function / Method, Pull Up Method, Form
  Template Method, Introduce Parameter Object. Confirm the duplication
  is *real* (same intent), not coincidental (license headers,
  generated code, similar boilerplate). Apply Rule of Three: if it's
  the second occurrence, leave it; you need three to inform the
  abstraction.
- **`change_coupling`** — Look for the hidden architectural seam. The
  fix is rarely "extract a helper"; it's usually "the boundary
  between A and B is wrong". Surface the trade-off to the user
  rather than guessing — this metric often signals a design call,
  not a refactor target.
- **`hotspot`** — Hotspot is a *flag*, not a problem. The actionable
  finding is the underlying CCN / duplication / coupling on the same
  file. Walk the file's other findings and pick from those.

For each finding, read the file before making the change. Don't trust
the summary alone — the metric might be measuring something that's
intentional (parser tables, exhaustive `match` arms, generated code).
If the finding is a false positive, log it in your session notes and
move on without committing.

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
