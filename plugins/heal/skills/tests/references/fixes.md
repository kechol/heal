# Test fix patterns

Loaded by `/heal:tests` while building proposals and applying them.
Three groups: **direct fixes** (low risk), **larger changes** (proposals
the user approves, often marked **needs a decision**), and **refusals**
(never, whatever the finding says).

## Direct fixes (risk: low)

### `coverage_pct` — unit tests for documented pure logic

When the function's behaviour is documented (rustdoc, JSDoc / TSDoc,
docstring, Go or Scala doc comment), it is pure logic (no clock, no
I/O, no global state), and a neighbouring test file already exists:
write tests in **Arrange / Act / Assert** form against the documented
contract — one test per case, grouped by behaviour, not by line.

### `change_coupling.drift` — align a drifted test

The source's recent commits (`git log -p -- <src>`) show one clear
contract change (a renamed field, a new signature, a new error variant)
and the test still uses the old shape: update its fixtures and
assertions to the current contract. Several contract changes since the
test last moved → a larger change.

### `skip_ratio` — a skip whose reason no longer holds

The reason is verifiable and stale: `#[ignore = "blocked on
docker-compose v2"]` when the compose file is v2 now; `skip(reason=
"awaiting issue #42")` when `gh issue view 42` shows it closed. Remove
the skip; the test must then pass, or the skip was load-bearing — undo
and propose instead. When the skipped test asserts a feature that no
longer exists, delete it rather than unskip it.

### `test_value` delete (confidence ≥ 0.9)

The test would pass with the behaviour its name claims broken — it
checks only its mocks, the framework, or constants. Delete it, one test
per commit. Coverage guard: with `[features.test.coverage]` enabled,
re-run the reporter and `heal status --refresh --feature test --json`;
if the source file's coverage dropped, keep the deletion only when the
note's detail says `checks=mock_values` or `checks=nothing`, and say so
in the commit message — otherwise undo. Without coverage configured,
propose the deletion for review instead of doing it.

### `test_duplicate`

`same_case` — delete the later test of the pair (the kept one covers the
same lines). `parameterizable` — merge both into one table-driven or
parameterized test in the project's existing style, keeping every input.

### `mock_scope` on a `pure_value`

Replace the mock with the real value.

## Larger changes (risk: medium or high)

Propose with the concrete tests or structure you intend to write.

- **`coverage_pct` on undocumented behaviour** — writing a test would
  cement a guess. Propose documenting the contract first and ask the
  user what it is (**needs a decision**).
- **`coverage_pct` on orchestration or I/O code** — unit tests with
  mocks prove only the wiring. Propose a small integration test with
  real collaborators, or a contract test at the boundary; which harness
  and fixtures is **needs a decision** when the project has none.
- **`change_coupling.drift` needing a rewrite** — the source's
  responsibility moved or was removed; propose deleting and rewriting
  the test against what exists now.
- **`skip_ratio` skipped more than 30 days** — the team chose to route
  around something; ask why before touching it (**needs a decision**).
- **`test_value` rewrite** — the test checks behaviour plus private
  calls or call counts; rewrite its assertions against observable
  behaviour. Never delete it.
- **`mock_scope` on the `subject` or an `internal_collaborator`** —
  mocks belong at process boundaries; propose restructuring the test
  (or the code's seams) so the collaborator runs for real.
- **Suite shape** — an inverted pyramid (integration tests covering pure
  logic that has no unit tests), a skip cluster in one suite, or the
  same drift recurring on one pair: propose rebalancing, splitting the
  suite, extracting a shared fixture, or retiring a test file.
- **Hotspot source without docs and without tests** — two gaps at once;
  propose the documentation step before the tests.

## Refusals

- **Weakening a test** — swapping strict assertions for loose ones,
  deep equality for truthiness, dropping fixture checks, adding
  `# pragma: no cover` / `#[allow(dead_code)]` /
  `/* istanbul ignore next */`, or deleting a failing test instead of
  fixing the bug or the test.
- **Skipping a flaky test** to lower `skip_ratio`; report the flake.
- **Tests that restate the implementation** (`assert add(2, 3) == 2 +
  3`) or assert what the code does rather than what the contract
  requires.
- **Tests you did not run.** Every commit carries a passing run.
- **Lowering `[calibration.coverage_pct]` thresholds** or disabling
  `[features.test]` to make findings go away.

## Verification

Run the project's test suite (`apply-loop.md` has the detection table)
and confirm it is green before every commit. When the change adds or
removes tests that coverage measures, re-run the coverage reporter
(`cargo llvm-cov --lcov --output-path lcov.info`, `pytest --cov …`, the
`post_commit_refresh` command in `.heal/config.toml`) so the next
`heal status --refresh` sees the new `lcov.info`.

With `[features.semantic]`, check tests you wrote or changed after
committing and before `heal mark fix`:

```sh
heal semantic ask --task verify_tests --diff HEAD~1..HEAD --json
```

When `tasks[0].result.pass` is `false` — a test checks only its mocks,
checks nothing, or mocks an internal collaborator — undo the commit
(`git reset --hard HEAD~1`) and rewrite the test against real behaviour.
Exit code 2 means skip this check.

## Commit message

- New tests: `test(<scope>): cover <symbol> in <file>`
- Drift: `test(<scope>): align <test> with <change>`
- Skips: `test(<scope>): re-enable <test>` / `test(<scope>): remove obsolete <test>`
- Removal: `test(<scope>): remove <test> (checked <what>)`

The body names the cause (the rename, the documented contract, the
stale skip reason), ends with a one-line runner summary
(`cargo test → 128 passed`), and carries one `Refs: F#<finding-id>`
trailer per targeted finding.
