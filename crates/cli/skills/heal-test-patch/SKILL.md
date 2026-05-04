---
name: heal-test-patch
description: Drain `[features.test]` findings from the cache, applying mechanical fixes (writing missing unit tests for uncovered hot paths, aligning drifted tests, re-enabling skipped tests whose reason no longer holds) one finding per commit. Refuses to start on a dirty worktree, runs the test suite for every commit, refuses to weaken assertions or skip flakes. Does NOT push or open PRs. Trigger on "fix the test findings", "drain the test cache", "add tests heal flagged", "/heal-test-patch".
---

# heal-test-patch

Drain the `[features.test]` findings that `heal status` produced.
One finding per commit, in Severity order, until the test slice of
the cache is empty (or the user stops). This is the **write**
counterpart to `/heal-test-review`.

## Mental model

`heal status --feature test --json` writes a `FindingsRecord` to
`.heal/findings/latest.json` and emits the test-family slice on
stdout. Findings produced by the `[features.test]` observer
family carry one of these `Finding.metric` strings:

- `coverage_pct` — uncovered source file (severity from
  `[calibration.coverage_pct]`).
- `change_coupling.drift` — `TestSrc` pair where the test isn't
  co-evolving with its source (Severity::Medium).
- `skip_ratio` — per-test-file skip percentage (`> 1% / > 5% / > 20%`
  bands).
- `test_hotspot` — per-src-file `commits × uncov_pct` decoration
  carrier; `Severity::Ok` itself, but flips `hotspot=true` on
  `coverage_pct` findings sitting on the same src.

Each id is deterministic — the same coverage gap on
`src/foo.rs::bar` keeps the same id across runs, so a finding that
disappears from the cache after a commit is genuinely fixed.

`/heal-test-patch` mirrors `/heal-code-patch` and `/heal-doc-patch`
verbatim — same pre-flight, same per-commit `heal mark fix`, same
constraints. The allow-list / escalate-list below is what makes the
test family different.

## Pre-flight (refuse to start when these fail)

1. **Clean worktree.** `git status --porcelain` must be empty. Stop
   otherwise.
2. **`[features.test]` enabled.** Probe with
   `heal status --feature test --json`. When the test family is
   disabled in `.heal/config.toml`, this command exits 1 with a
   stderr message naming the missing config switch — that's the
   early-exit contract. Bail and tell the user to run
   `/heal-setup` (or hand-edit `.heal/config.toml`) to enable the
   feature before retrying.
3. **Coverage data reachable.** When `lcov.info` is missing for
   `coverage_pct` findings, recommend `/heal-test-reporter-setup`
   first. Drift / skip findings can still drain without it.
4. **Cache exists.** `heal status --feature test --json` returns at
   least one `[features.test]` finding. If only `severity: "ok"`
   test findings exist, say so — the calibration thresholds are
   loose enough that nothing is actionable.
5. **Test runner detected.** Verify the project has a runnable test
   suite (`cargo test`, `pytest`, `npm test`, `go test`,
   `sbt test`). Without one, the verification step below can't
   pass and the loop must not commit.

## The loop

```
while there are non-Ok [features.test] findings in the cache:
    pick the next one (Severity order: Critical🔥 → Critical → High🔥 → High → Medium)
        skip findings where `accepted == true`
    decide: allow-list (apply mechanically) or escalate-list (stop)?
    if allow-list:
        read the source / test; apply the smallest fix
        run the project's test runner — must be green
        git commit -m "<conventional test message>"
        heal mark fix --finding-id <id> --commit-sha <sha>
        heal status --refresh --feature test --json
    if escalate-list:
        end the session; surface remaining findings; recommend /heal-test-review
```

Stop conditions: test slice of cache empty, user interrupts, or
only escalate-list findings remain.

## Allow-list (apply mechanically)

These transformations are deterministic enough to apply in-loop
after reading the source / test to confirm the pattern fits.

### `coverage_pct` — write a unit test for a hot path with documented behavior

Apply when **all** of these hold:

- The function's behavior is documented (rustdoc, JSDoc / TSDoc,
  Python docstring, Go package comment, ScalaDoc) — you have a
  contract to assert against.
- The function is pure-logic: no clock, no I/O, no global state.
- A test file already exists in one of `[features.test].test_paths`
  for the source's neighbor — you're adding a test, not founding a
  test directory.

Fix shape: write the test using **AAA structure** (Arrange / Act /
Assert) — set up inputs / fixtures, call the function under test
exactly once, assert against the documented contract. One test per
case; group cases by behavior, not by line of code.

Run the project's test runner *before* committing. The commit
message body must include the runner output (or a one-line
`cargo test → ok. 124 passed.` summary).

### `change_coupling.drift` — align a drifted test

The test asserts behavior the source no longer has. Apply when:

- Reading the source's recent commits (`git log -p -- <src>`)
  surfaces a clear contract change (renamed field, signature change,
  new error variant) AND the test's assertions / fixtures still
  reference the old shape.
- Updating the assertions to match the source's current contract
  is mechanical: rename a field, update an expected-value string,
  swap an error variant.

Don't auto-pick when the source has had multiple contract changes
since the test was last touched — escalate.

### `skip_ratio` — re-enable a skipped test whose reason is stale

Apply when the skip's reason string is verifiable AND the reason no
longer holds. Examples:

- `#[ignore = "blocked on docker-compose v2"]` — `docker-compose.yml`
  now uses v2 syntax; remove the `#[ignore]`.
- `@pytest.mark.skip(reason="awaiting issue #42")` — issue #42 is
  closed (verify via `gh issue view 42`).
- `it.skip("legacy API removed in v0.3")` — the source's legacy API
  is removed; this test has nothing left to assert. Delete the
  test entirely (don't unskip what's no longer relevant).

After re-enabling, run the test runner. The test must pass — if it
fails, the skip was load-bearing; revert and escalate.

## Escalate-list (stop and surface)

These need judgment that lives in `/heal-test-review` (or with the
user). When a finding's best-fit pattern is here, end the loop:

### `coverage_pct` on undocumented behavior

The function has no docstring, no module-level explanation, and no
prior tests to imitate. Writing a test would require *guessing*
the intended behavior, which encodes the guess as canonical and
makes future bugs invisible. Surface the documentation gap to
`/heal-test-review` and stop.

### `coverage_pct` on I/O / orchestration code

Functions that compose other modules but contain little branching
of their own. A unit test with mocks proves the mocks are wired
up; an integration test belongs in a different harness. The choice
of which harness, what fixtures, how much real-vs-fake I/O — all
architectural. Escalate.

### `change_coupling.drift` requiring rewrite

The source has fundamentally changed (a feature was removed, the
function's responsibility migrated elsewhere). The test asserts
behavior nothing in the codebase has anymore. The right fix is
delete-and-rewrite, which is an architectural call. Escalate.

### `skip_ratio` skipped > 30 days

Treat as architectural decision, not mechanical. The team chose to
route around something; that choice has a context this skill
doesn't have. Surface for human review (and for
`/heal-test-review` to interpret as a possible suite-architecture
signal).

### Anything paired with a hotspot decoration on undocumented code

A `coverage_pct` Critical on a hotspot file with no docstrings
combines two distinct gaps. Mechanical test-writing here cements a
guess into the suite. Escalate to `/heal-test-review` for
proposal-level discussion before mechanical fixes.

## Forbidden anti-patterns (must refuse)

These are not "stop and ask" — they are bright-line refusals. If a
fix's only path forward goes through one of these, abort the
finding and move to the next.

### Lowering test strength to drain a finding

- Replacing a strict assertion (`assert_eq!(out, Expected::Foo)`)
  with a loose one (`assert!(out.is_ok())`) to make a test pass.
- Replacing a deep equality check with a shallow truthiness check.
- Weakening fixture validation: dropping schema checks, broadening
  regex matchers, removing length / count assertions.
- Adding `# pragma: no cover` / `#[allow(dead_code)]` /
  `/* istanbul ignore next */` to suppress coverage measurement
  rather than actually covering the code.
- Deleting a test that fails (as opposed to fixing the bug or the
  test).

The point of the suite is to detect regression. Every weakening
above narrows that detection window. The metric will move; the
codebase's safety net won't.

### Adding `@pytest.mark.skip` / `#[ignore]` / `it.skip` to "fix" a flake

Flakiness is **out of scope** for this skill. Skipping a flake is
the wrong remediation: it converts an intermittent signal into a
permanent blind spot, AND raises the `skip_ratio` metric this skill
is supposed to drain. Surface the flake to the user and move on.

### Generating tests without running them

Every patch commit must include a successful test-runner output.
"I wrote the test; the user can run it" is not acceptable — the
loop's contract is one-finding-per-commit-with-passing-tests. If
the runner won't run (missing dependency, broken harness), the
finding is in escalate territory.

### Bulk-adding scaffolded tests across multiple files in one commit

The contract is **one finding per commit**. A coverage drain across
12 files is 12 commits, not one. Bulk commits break `heal diff`'s
ability to attribute drops, break the audit trail in `fixed.json`,
and make it impossible to revert a single bad test in isolation.

### Manufacturing assertions

A test that asserts what the implementation does (rather than what
the contract requires) is worse than no test — it locks in the
current behavior, including its bugs. When you can't articulate
*what should be true regardless of how the function is written*,
the finding is undocumented behavior and belongs in escalate.

## Verification per commit

Run the project's test runner and confirm green before every
commit. Best-effort detection (same as `/heal-code-patch`):

- `Cargo.toml` → `cargo test` (or the workspace flavor:
  `cargo test --workspace`).
- `package.json` with a `test` script → `npm test` / `pnpm test` /
  `yarn test`.
- `pyproject.toml` with a `pytest` section → `pytest`.
- `go.mod` → `go test ./...`.
- `build.sbt` → `sbt test`.

For `coverage_pct` allow-list fixes, also re-run the coverage
reporter (`cargo llvm-cov --lcov --output-path lcov.info`,
`pytest --cov ...`, etc.) so the next `heal status --refresh`
sees the updated `lcov.info`. Without that, the finding will
appear unfixed even though the test exists.

If verification fails, **revert the change** (`git restore .`) and
skip the finding.

## Commit message format

Conventional Commits with the finding id as the trailing tag:

```
test(heal): cover validate_order in src/payments/engine.rs

Add four AAA-structured unit tests against the function's
documented contract: empty cart returns ValidationError::Empty,
negative quantity returns ValidationError::Quantity, etc. Coverage
on engine.rs climbs from 12% to 78%.

cargo test → 128 passed.

Refs: F#coverage_pct:src/payments/engine.rs:1234567890abcdef
```

Subject: `test(heal): <verb> <symbol> in <file>` for new tests,
`test(heal): align <test-name> in <test-file>` for drift fixes,
`test(heal): re-enable <test-name>` (or `test(heal): remove obsolete
<test-name>`) for skip resolutions.

Body: 2-3 sentences including the underlying cause (rename,
documentation, stale skip reason) and a one-line runner-output
summary. The audit-trail value is "why was this fix correct?", not
just "what changed."

## Marking the commit

```sh
heal mark fix \
  --finding-id "<finding_id>" \
  --commit-sha "$(git rev-parse HEAD)"

heal status --refresh --feature test --json
```

Same pattern as `/heal-code-patch` and `/heal-doc-patch`.

## Output format

While running, narrate one short paragraph per finding:

```
[1/8] 🔴 Critical 🔥  coverage_pct  src/payments/engine.rs (12% → 78%)
  validate_order documented in module rustdoc; wrote 4 AAA unit
  tests against the documented errors. cargo test → 128 passed.
  Committed: a1b2c3d4. heal status confirms fixed.

[2/8] 🟡 High        skip_ratio  tests/parser_test.rs (18%)
  Three skips referencing "legacy syntax removed in v0.3"; the
  legacy syntax is gone, the tests have nothing left to assert.
  Removed both tests and the now-empty fixture. cargo test → green.
  Committed: e7f8g9h0. heal status confirms fixed.
```

End with a session summary:

```
Test cache drain: fixed 6 / skipped 1 / regressed 0 / 1 escalated.
Escalated: src/protocol/handshake.rs coverage_pct (Crit🔥) — function
has no documented behavior, writing tests would encode a guess.
Recommend running /heal-test-review for proposal-level discussion.
```

## Constraints

- One finding = one commit. Don't bundle multiple findings.
- **Never push.** Local commits only; user runs `git push`.
- **Never amend.** New commit per finding is the contract.
- **Never `--no-verify`.** Fix the underlying issue or skip.
- **Never weaken assertions** to drain a finding (Coverage trap).
- **Never skip a flake** to drain a `skip_ratio` finding (the
  metric is the proxy, not the target).
- **Never commit without running tests.** Every patch commit
  carries runner output.
- **Never bulk-commit.** One finding per commit; the audit trail
  in `fixed.json` depends on it.
- **English commit messages.** The test itself may use any
  language for fixture data; the commit message stays English
  (workflow.md R6.1).
