---
title: Test · Skills
description: The heal plugin's skill for [features.test] — /heal:tests, which reviews and improves your test suite — and the /heal:setup step that wires up coverage.
---

The opt-in **Test** family is served by two skills from the heal
Claude Code plugin: the tests step of `/heal:setup`, which enables the
family and wires up a coverage reporter, and `/heal:tests`, which works
on the tests themselves. For installing the plugin, see
[Code › Skills](/heal/code/skills/).

## `/heal:setup tests` — enable the family and wire up lcov

Turns `[features.test]` on with `test_paths` and `lcov_paths` fitted to
your repository, then detects your language stack and sets up a
coverage reporter so `lcov.info` lands where heal reads it.

| Stack   | Reporter                                                                   |
| ------- | -------------------------------------------------------------------------- |
| Rust    | `cargo llvm-cov --lcov --output-path lcov.info`                            |
| Python  | `pytest --cov=src --cov-report=lcov` (`pytest-cov`)                        |
| JS / TS | `nyc --reporter=lcov mocha` / `vitest --coverage --coverage.reporter=lcov` |
| Go      | `go test -coverprofile=coverage.out` + `gcov2lcov`                         |
| Scala   | `scoverage` plugin + lcov reporter                                         |
| Mixed   | one reporter per stack                                                     |

Every step — installing the reporter, editing `.heal/config.toml`,
running the reporter — asks first. It can also set
`[features.test.coverage].post_commit_refresh` so the post-commit hook
refreshes `lcov.info` after each commit. CI changes are printed as a
proposal to copy, not applied. `heal doctor` reports the test area as
`todo` when coverage is on but no lcov file exists, so `/heal:setup`
offers the step again.

## `/heal:tests` — review and improve the tests

Works like `/heal:refactor`, for tests. It reads the test findings,
proposes changes, and applies only the ones you approve — one commit
per proposal, with the test suite green before every commit. It never
pushes or opens a pull request.

1. **Diagnose.** Reads `heal status --all --feature test --json`, then
   each flagged source file with its tests. Uncovered code is sorted by
   the layer that should test it, following the **test pyramid**: pure
   logic gets unit tests; code that coordinates other modules gets a
   small integration test; code at an I/O boundary gets a thin contract
   test. Skips are sorted by their reason (environment, slow, broken,
   pending).
2. **Propose.** A short reading of the suite's shape — an inverted
   pyramid, hotspots without tests, skips piling up, tests that no
   longer follow their source — then numbered proposals,
   highest-priority findings first. Each test proposal names the cases
   it will assert.
3. **Choose.** You pick which proposals to apply.
4. **Apply.** One commit per proposal; the suite must pass, and the
   coverage reporter is re-run when coverage changes.
5. **Report.** What changed and what is left.

What it proposes, by finding:

| Metric                  | Typical proposal                                                                                          |
| ----------------------- | --------------------------------------------------------------------------------------------------------- |
| `coverage_pct`          | Unit tests for documented pure logic; an integration or contract test for coordination and I/O code.      |
| `change_coupling.drift` | Update the test to the source's current contract, or rewrite it when the behaviour moved elsewhere.       |
| `skip_ratio`            | Re-enable a test whose skip reason no longer holds; delete one whose feature is gone; ask about the rest. |

When the same problem repeats across files it proposes **suite-level**
changes too: rebalancing an inverted pyramid, splitting a suite,
extracting a shared fixture, or retiring a test file. Code whose
intended behaviour is not documented anywhere is raised as a question —
writing a test would only cement a guess.

**Refusals:** never weakens an assertion to make a test pass, never
skips a flaky test to lower `skip_ratio`, never commits a test it has
not run, and never lowers coverage thresholds or turns the family off
to make findings go away.

**With [Semantic (Jev)](/heal/semantic/) enabled**, it can also remove
tests that check nothing — a `test_value` finding marked `delete` at
high confidence, one test per commit, only while the suite passes and
coverage does not drop (unless the test only checked its own mocks);
merge or remove duplicated tests (`test_duplicate`); and replace mocks
of plain values with the real values (`mock_scope`). After each commit
it runs `heal semantic ask --task verify_tests --diff HEAD~1..HEAD` and
undoes a commit whose new test only checks its own mocks.

Arguments: a path narrows the work to findings under it, a finding id
narrows it to that finding, and `plan` stops after the proposals.

Trigger phrases: "review the test health", "fix the test findings",
"add the tests heal flagged", "remove useless tests", "/heal:tests".
