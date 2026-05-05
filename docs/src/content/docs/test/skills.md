---
title: Test · Skills
description: The three bundled skills for [features.test] — /heal-test-reporter-setup, /heal-test-review, /heal-test-patch. Available for Claude Code and OpenAI Codex.
---

The opt-in **Test** family ships three skills, extracted alongside
the Code-family skills for every detected agent target on
`heal init`. They only act on findings produced by the test
observers.

For installation and the drift-aware update model, see
[Code › Skills](/heal/code/skills/) — the mechanism is shared.

## `/heal-test-reporter-setup` — wire up lcov

One-shot setup skill. Detects your project's language stack and
proposes the lcov reporter configuration plus CI integration so
`lcov.info` lands at one of heal's default `lcov_paths`.

| Stack | Proposes |
|---|---|
| Rust | `cargo llvm-cov --lcov --output-path lcov.info` |
| Python | `pytest --cov=src --cov-report=lcov` (`pytest-cov`) |
| JS / TS | `nyc --reporter=lcov mocha` / `vitest --coverage --coverage.reporter=lcov` |
| Go | `go test -coverprofile=coverage.out` + `gcov2lcov` |
| Scala | `scoverage` plugin + lcov reporter |
| Mixed | per-stack proposals + a CI step producing a single `lcov.info` |

Read-only on the codebase — proposes commands and config edits
without running them. You run the commands; heal reads the
resulting `lcov.info` on the next `heal status`.

Trigger phrases: "set up coverage reporting", "configure lcov for
heal", "wire up coverage", "/heal-test-reporter-setup".

## `/heal-test-review` — the audit skill

Read-only. Reads `heal status --json`, filters to the
`[features.test]` slice, and returns:

1. An **architectural reading** of the test suite — is the
   dominant axis "no unit tests", "tests aren't keeping up with
   their source", "important paths uncovered", or "skipped flakes
   that solidified into permanent skips"?
2. A **prioritized test-fix TODO list** — coverage gaps on hotspot
   files first, then drifting tests, then skip-ratio outliers.

Never edits source. After reading the review you can act on any
item right away — ask the agent in the same session ("write the
missing tests for the top three", "re-enable the skipped tests
under `auth/`"). Mechanical fixes flow through
`/heal-test-patch`; judgment calls — should this skip stay
because the underlying flake is real? is this uncovered file
something we test, or something we delete? — wait for your
direction.

### Why review and patch are split

**Patch** handles the mechanical: write a unit test for a clearly
uncovered branch, sync a drifted test back to its source, unskip
a test whose recorded reason ("waiting on issue #123") is now
resolved. **Review** also surfaces the judgment calls —
suspiciously weak assertions, skips that probably hide a real
flake, files that maybe shouldn't be tested at this layer at all.
Mixing the two would either paper over a real flake or refuse to
write the easy tests.

Trigger phrases: "review the test health", "where should we add
tests", "which tests should we unskip", "/heal-test-review".

## `/heal-test-patch` — the write skill

Drains the test slice of `.heal/findings/latest.json` one finding
at a time, in Severity order. **One commit per fix.**

**Pre-flight** (refuses to start otherwise):

- Clean worktree.
- Cache exists (runs `heal status --json` to populate if missing).
- `[features.test]` enabled in `.heal/config.toml`.
- `lcov.info` present at one of `lcov_paths` when `coverage_pct`
  findings are in scope (otherwise the skill points the user at
  `/heal-test-reporter-setup`).

**Per-metric moves:**

| Metric | Default move |
|---|---|
| `coverage_pct` | Write or extend a unit test for an uncovered hot path; re-run the reporter so `lcov.info` updates. |
| `skip_ratio` | Re-enable a skipped test whose reason no longer holds; remove the skip marker, run the test, fix any failure inline. |
| `change_coupling.drift` | Align the drifted test with its source — the skill surfaces both files together. |

**Refusals** (encoded in the skill body, won't budge with
prompting):

- **Assertion-weakening** — never converts `assert.equal(x, 5)` to
  `assert.ok(x)` to pass a stale test. If the assertion is wrong,
  the test gets removed with a commit message naming the reason.
- **Skip-the-flake** — never adds a skip marker to make a flaky
  test go away.
- **Scaffold-without-running** — every commit runs the test
  suite. Tests that "look right" but haven't been executed never
  land.

**Constraints**: one finding = one commit, Conventional Commit
subject + `Refs: F#<finding_id>` trailer, never push / amend /
`--no-verify`. Findings whose metric belongs to the Code or Docs
families are skipped.

Trigger phrases: "fix the test findings", "drain the test cache",
"add tests heal flagged", "/heal-test-patch".
