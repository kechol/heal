---
title: Test · Skills
description: The three bundled Claude Code skills for [features.test] — /heal-test-reporter-setup, /heal-test-review, /heal-test-patch.
---

The opt-in **Test** family ships three Claude skills, extracted
alongside the Code-family skills on `heal skills install` /
`heal init`. They only act on findings produced by the test
observers.

For installation and the drift-aware update model, see
[Code › Skills](/heal/code/skills/) — the mechanism is shared.

## `/heal-test-reporter-setup` — wire up lcov

One-shot setup skill. Detects your project's language stack
(Rust / Python / JS-TS / Go / Scala / mixed) and proposes the
lcov reporter configuration plus CI integration so `lcov.info`
lands at one of heal's default `lcov_paths`.

| Stack | Proposes |
|---|---|
| Rust | `cargo install cargo-llvm-cov` + `cargo llvm-cov --lcov --output-path lcov.info` |
| Python | `pytest --cov=src --cov-report=lcov` (via `pytest-cov`) |
| JS / TS | `nyc --reporter=lcov mocha` / `vitest --coverage --coverage.reporter=lcov` |
| Go | `go test -coverprofile=coverage.out` + `gcov2lcov` |
| Scala | `scoverage` plugin + lcov reporter |
| Mixed | per-stack proposals + a CI step that produces a single `lcov.info` |

Read-only on the codebase — proposes commands and config edits
without running them. You run the commands; heal reads the
resulting `lcov.info` on the next `heal status`.

Trigger phrases: "set up coverage reporting", "configure lcov for
heal", "wire up coverage", "/heal-test-reporter-setup".

## `/heal-test-review` — the audit skill

Read-only. Reads `heal status --json`, filters to the
`[features.test]` slice (using `Finding.metric` and
`Finding.is_test_file`), and returns:

1. An **architectural reading** of the test suite. Is the dominant
   axis "no unit tests", "tests aren't keeping up with their
   source", "important paths uncovered", or "skipped flakes that
   solidified into permanent skips"?
2. A **prioritized test-fix TODO list** — coverage gaps on hotspot
   files first, then drifting tests, then skip-ratio outliers.

`/heal-test-review` proposes only — it never edits source. The
write counterpart is `/heal-test-patch`.

Trigger phrases: "review the test health", "where should we add
tests", "which tests should we unskip", "/heal-test-review".

## `/heal-test-patch` — the write skill

Drains the test slice of `.heal/findings/latest.json` one finding
at a time, in Severity order. **One commit per fix.**

Pre-flight (refuses to start when these fail):

1. Clean worktree.
2. Cache exists (runs `heal status --json` to populate if missing).
3. `[features.test]` enabled in `.heal/config.toml`.
4. `lcov.info` exists at one of `lcov_paths` when `coverage_pct`
   findings are in scope. If not, the skill points the user at
   `/heal-test-reporter-setup`.

Per-metric drain pattern:

| Metric | Default move |
|---|---|
| `coverage_pct` | Write or extend a unit test for an uncovered hot path. Re-runs the coverage reporter per commit so `lcov.info` updates. |
| `skip_ratio` | Re-enable a skipped test whose reason no longer holds. Removes the skip marker, runs the test, fixes any failure inline. |
| `change_coupling.drift` | Align the drifted test with its source. The skill surfaces both files together. |

Refusals encoded in the skill body — these won't budge even with
prompting:

- **Assertion-weakening** — never converts `assert.equal(x, 5)` to
  `assert.ok(x)` to pass a stale test. If the assertion is wrong,
  the test gets removed with a commit message naming the reason,
  not silently relaxed.
- **Skip-the-flake** — never adds a skip marker to make a flaky
  test go away. Flakiness is its own problem and belongs in a
  separate fix.
- **Scaffold-without-running** — every commit runs the test suite
  (or the per-language equivalent). Tests that "look right" but
  haven't been executed never land.

Constraints (enforced by the skill):

- One finding = one commit.
- Conventional Commit subject + body + `Refs: F#<finding_id>`
  trailer.
- Never push, never amend, never `--no-verify`.

`/heal-test-patch` skips findings whose metric belongs to the Code
or Docs families.

Trigger phrases: "fix the test findings", "drain the test cache",
"add tests heal flagged", "/heal-test-patch".
