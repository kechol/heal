# Apply loop

Shared mechanics for the three work skills — `/heal:refactor` (code),
`/heal:docs`, and `/heal:tests`. Each of them runs the same five steps:

1. **Diagnose** — read heal's findings and the files behind them
   (family-specific; in the skill body).
2. **Propose** — a short reading of the whole picture plus numbered
   proposals (family-specific; in the skill body).
3. **Choose** — the user approves which proposals to apply (this file).
4. **Apply** — one approved proposal per commit (this file).
5. **Report** — what changed, what was skipped, what is left (this file).

Nothing is written to the working tree before step 3. Steps 1–2 are
read-only and may run on a dirty worktree.

## Proposals

A proposal is one coherent change that removes real friction — code or
prose that is hard to read, hard to test, or hard to change. It may
resolve several findings at once (a file split can clear a
`concept_mix`, an `lcom`, and two `ccn` findings together) and may touch
several files. Findings are the evidence for a proposal, never its goal:
a proposal whose only benefit is a lower number is not worth making.

Number every proposal and give each one:

- **Change** — what moves where, with a named pattern when one fits.
- **Why** — the friction it removes, in one or two sentences.
- **Resolves** — the finding ids it targets (exact ids from the JSON).
- **Touches** — the files, including tests and docs that must change
  with it.
- **Risk** — `low` (local, behaviour-preserving, well tested), `medium`
  (several files or thin tests), or `high` (public API, data format,
  cross-module boundary). Mark a proposal **needs a decision** when it
  depends on a choice only the user can make (a public name, which side
  of a boundary owns a responsibility, whether a behaviour is intended).

## Choose

Show the numbered proposals, then ask with `AskUserQuestion`
(`multiSelect: true`): offer the strongest proposals as options (up to
four) and let the user type any other numbers, or "none", via Other.

- Apply only what the user selected, in the order listed.
- A proposal marked **needs a decision** gets its own question with the
  concrete alternatives before it is applied, even when the user said
  "all".
- If the user selects nothing, stop after the report. The proposals
  stay useful as a plan.

## Pre-flight (before the first write)

1. **Clean worktree.** `git status --porcelain` must print nothing. If
   it does, stop and ask the user to commit or stash — one commit per
   proposal needs a clean baseline, and reverting a failed attempt must
   never touch their work.
2. **Find the verification commands** (see "Verification" below) and
   run them once. If they already fail on the clean tree, tell the user
   and ask whether to continue; you cannot tell your breakage from
   theirs otherwise.

## Apply one proposal

1. Re-read the files. Earlier proposals in this session may have
   changed them.
2. Make the change. Keep it to what the proposal says; update callers,
   imports, tests, and docs that the change would otherwise leave wrong,
   in the same commit.
3. Verify (below). On failure, discard the attempt with
   `git reset --hard HEAD && git clean -fd` (safe because pre-flight
   guaranteed a clean tree), note why, and move to the next proposal.
   Never commit a failing build.
4. Commit (format below). Never `--no-verify`, never amend.
5. With `[features.semantic]`, check the commit:

   ```sh
   heal semantic ask --task verify_patch --diff HEAD~1..HEAD --json
   ```

   Read `tasks[0].result`. When `pass` is `false` — `flags` contains
   `relocate` (complexity moved, not removed), `guard_clause` (a flat
   condition flipped into negated early returns), or `message_mismatch`
   — or when `checks.behaviour_change ≥ 0.7` for a change meant to keep
   behaviour, undo it with `git reset --hard HEAD~1`, note why, and move
   on. Exit code 2 (no key, or the family is off) means skip this check.
6. Record every finding the proposal targeted:

   ```sh
   heal mark fix --finding-id "<id>" --commit-sha "$(git rev-parse HEAD)"
   ```

   One call per finding id, all with the same SHA.
7. Refresh the family's view:

   ```sh
   heal status --refresh --feature <code|docs|test> --json
   ```

   Targeted ids that are gone are fixed. An id still present will show
   up as regressed on the next run; note it and continue. **Stop and
   tell the user** when new Critical or High findings appeared in the
   files you just touched — the change relocated the problem instead of
   removing it.

## Accept instead of change

Some findings are the metric counting something it should not: a
generated file, an exhaustive `match` over a closed enum, a coherent
pipeline, a test that is intentionally skipped outside CI. Propose
recording them as accepted rather than changing the code:

- Ask with `AskUserQuestion` (up to four findings per question,
  `multiSelect: true`), naming the file, the metric, and the one-line
  reason for each.
- On approval, run
  `heal mark accept --finding-id "<id>" --reason "<categorical_reason>"`.
- Never accept without approval, and never accept a finding just
  because it is hard to fix — that is a proposal marked **needs a
  decision**, or a deferred question.

Use a stable, categorical reason so later audits can group accepts.
Reasons in use:

| Family | Reason | When |
|---|---|---|
| code | `generated_code` | parser tables, schema-derived types, generated bindings |
| code | `vendored_third_party` | vendored dependencies |
| code | `exhaustive_enum_dispatch` | a `match` / `switch` over a closed enum; splitting loses exhaustiveness |
| code | `intentional_parser_table` | data-shaped tables that read best as one block |
| code | `coherent_pipeline_relocate_trap` | sequential phases; extracting only relocates |
| code | `stateless_delegation` | LCOM on a field-less type whose methods delegate (LCOM's blind spot) |
| docs | `false_positive_observer_extracted_non_codebase_identifier` | `doc_drift` matched a common word, a third-party or stdlib name |
| docs | `pair_coverage_gap_identifier_exists_in_unpaired_src` | the identifier exists in a src not listed for the doc — fix `.heal/doc_pairs.json` via `/heal:setup` |
| docs | `false_positive_observer_slugify_diverges_from_github_slugger` | the anchor works in the site generator but the observer's slug rule differs |
| docs | `false_positive_observer_counts_noun_phrase_TODO` | "the TODO list" and similar noun phrases |
| docs | `intentional_external_link` | a link meant to resolve outside the source tree |
| test | `coverage_exercised_via_integration_suite` | covered by a suite the lcov reporter does not see |
| test | `intentional_skip_environment_gated` | skipped outside CI on purpose (hardware, paid APIs) |
| test | `change_coupling_drift_test_lives_at_higher_layer` | an architecture or integration test that should not co-evolve per commit |
| test | `coverage_pct_generated_or_vendored` | generated or vendored code |

Coin a new reason only when none fits; keep it snake_case and durable.

## Verification

Detect the project's commands; prefer what CI runs.

| Signal | Build / type-check | Tests |
|---|---|---|
| `Cargo.toml` | `cargo check --all-targets` | `cargo test` (`--workspace` in a workspace) |
| `package.json` | `tsc --noEmit` when `tsconfig.json` exists | the `test` script via the lockfile's package manager |
| `pyproject.toml` / `setup.py` | `mypy .` when configured | `pytest` |
| `go.mod` | `go vet ./...` | `go test ./...` |
| `build.sbt` | `sbt compile` | `sbt test` |

Also run the project's formatter and linter when CI enforces them. The
family skills add their own checks (link re-checks for docs, the
coverage reporter for tests).

## Commit message

Follow the project's own convention (read `git log --oneline -20`).
Without one, use Conventional Commits in English:

```
refactor(payments): split order validation out of processOrder

processOrder mixed input validation with pricing, so every pricing
change had to re-read forty lines of checks. Validation now lives in
validate_order next to its tests.

Refs: F#ccn:src/payments/engine.ts:processOrder:9f8e7d6c5b4a3210
Refs: F#lcom:src/payments/engine.ts:OrderEngine:0123456789abcdef
```

- Type: `refactor` (behaviour kept), `fix` (a bug), `docs`, `test`.
- Body: the friction removed, not the metric movement.
- One `Refs: F#<finding-id>` trailer per targeted finding.

## Never

- Push, open a pull request, amend, or skip hooks.
- Weaken a test, delete an assertion, or skip a flaky test to make a
  change pass.
- Recalibrate, edit `.heal/calibration.toml` or `.heal/findings/*` by
  hand, or run `heal mark` for work that was not committed.
- Extend beyond the approved proposals. New ideas go in the report.

## Report

While applying, narrate one short paragraph per proposal:

```
[2/4] Split order validation out of processOrder  (resolves 3)
  cargo test → 212 passed. verify_patch → pass. Committed 1a2b3c4.
  heal status: 3 of 3 targeted findings gone.
```

End with:

```
Applied 3 of 4 proposals (3 commits, 7 findings resolved); 1 skipped: <why>.
Accepted 2 findings.
Left in the queue: T0 4, T1 9, advisory 31.
Next: review with `git log --oneline -3`, push when ready.
```
