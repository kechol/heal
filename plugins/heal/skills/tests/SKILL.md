---
name: tests
description: Work on the project's tests from what heal's `[features.test]` family found — uncovered code on files that change often, tests drifting from the code they cover, skipped tests piling up, and (with `[features.semantic]`) tests that check nothing but their own mocks, mocks in the wrong place, and duplicated tests. Reads the suite through the test pyramid, proposes tests to write, align, re-enable, merge, or delete, and applies the ones the user approves, one commit per proposal with the suite green. Never weakens an assertion, skips a flake, pushes, or opens a PR. Trigger on "review the test health", "what does heal say about my tests", "fix the test findings", "add the tests heal flagged", "remove useless tests", "which tests should we unskip", "/heal:tests".
argument-hint: "[path | finding-id | plan]"
---

# /heal:tests

Load before starting:

- `${CLAUDE_PLUGIN_ROOT}/references/cli.md` — CLI contract, output
  language.
- `${CLAUDE_PLUGIN_ROOT}/references/apply-loop.md` — proposal format,
  approval, the per-commit loop, accepts, and the report.
- `references/fixes.md` — how each finding is fixed, and what never to
  do.

Arguments: a path narrows to findings under it; a finding id narrows to
the proposal that resolves it; `plan` stops after proposing.

The target is not 100% coverage, zero skips, or zero drift. It is that
the code which changes most has a safety net that would catch a broken
behaviour — Critical findings on hotspot files first. Tests written to
move a number tend to assert the type system; skips removed under
pressure come back as flakes.

## 1. Diagnose

1. `heal status --all --feature test --json`. Exit 1 means the family is
   disabled — suggest `/heal:setup tests` and stop. When coverage is
   enabled but `coverage_observation.state` is not `complete`, say which
   sources are missing or unreadable before reading `coverage_pct`. Set
   aside accepted findings; mention `accepted_rereview` as notes.
2. Take the queue: T0 then T1 in ascending `drain_rank` (counted within
   the Test family). `is_test_file: true` anchors a finding on a test
   (the fix lives in the test); otherwise it sits on production code
   (the fix is a new or changed test).
3. Read each source and its tests. Classify uncovered code by the layer
   that should test it:
   - **Pure logic** (no I/O, no clock, no global state) — unit tests;
     cheap, highest leverage; most Critical `coverage_pct` on hotspots.
   - **Coordination** (composes modules, little branching) — a small
     integration test with the real collaborators; mocking them proves
     nothing.
   - **I/O boundary** (database, HTTP, filesystem) — a thin contract
     test, and unit tests inside the layer it feeds.
4. Classify every skip by its reason: environment (usually fine), slow
   (fine if it still runs on demand), broken (the actionable kind),
   pending (a TODO, not a skip to silence).
5. For `change_coupling.drift`, compare the source's and the test's
   recent history (`git log --since="6 months ago" -- <file>`); thirty
   source commits against zero test commits is a stale test.
6. Look at the suite's shape: pure logic covered only by slow
   integration tests (an inverted pyramid), hotspots without tests,
   skip clusters, the same pair drifting again and again.

### With `[features.semantic]`

Read every note by its confidence: at `≥ 0.9` act on it, at `0.5–0.9`
confirm it first, below `0.5` ignore it.

- `test_value` — a test that would pass with its claimed behaviour
  broken (`delete`) or tied to implementation details (`rewrite`); the
  detail lists all four answers. Agent-written suites accumulate these
  fastest.
- `mock_scope` — a mock of the code under test (`subject`) or of an
  internal collaborator; mocks belong at process boundaries.
- `test_duplicate` — near-identical tests (`same_case`) or ones that
  differ only in data (`parameterizable`).
- `semantic.coverage_band` on `coverage_pct` — `pure_logic`,
  `coordination`, or `io_boundary`, the classification in step 3.
- `semantic.skip_reason` on `skip_ratio` — `environment`, `slow`,
  `broken`, or `pending`, with each skipped test in the detail.
- `semantic.gate` and `semantic.effort` are a second opinion, never the
  decision.

## 2. Propose

An **architectural reading** of three to six lines: where the pyramid is
out of shape, which hotspots lack a net, and whether the suite carries
dead weight (skips, tests that check nothing, duplicates).

Then up to eight numbered **proposals** in the `apply-loop.md` format,
drawing on `references/fixes.md`: tests to write for pure logic on
hotspots, drifted tests to align or rewrite, skips to resolve, tests to
delete or merge, mocks to move to the boundary, and suite-shape changes
(rebalance the pyramid, split a suite, extract a shared fixture) when
the evidence repeats across files. For each test proposal, name the
cases it will assert. Mark **needs a decision** where `fixes.md` says
so. Show accept candidates (`apply-loop.md` has the test reasons) and
deferred questions after the proposals. With `plan`, stop here.

## 3–5. Choose, apply, report

Follow `apply-loop.md`, with the verification and commit format from
`references/fixes.md`. Every commit carries a passing run of the suite;
refuse the patterns under "Refusals" even when a proposal seems to call
for them.

End the report with the next step: another `/heal:tests` round,
`/heal:setup tests` when coverage is not wired yet, or `/heal:refactor`
when code is hard to test because of its structure (separating logic
from its dependencies is a code change, not a test change).
