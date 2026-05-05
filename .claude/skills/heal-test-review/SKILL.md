---
name: heal-test-review
description: Read every finding from the `[features.test]` observer family produced by `heal status --feature test --json`, deeply investigate the user's tests and codebase through the test-pyramid lens, and return one architectural reading plus a prioritized test-fix TODO list. Read-only — proposes only. The write counterpart is `/heal-test-patch`. Trigger on "review the test health", "what does heal say about my tests", "where should we add tests", "which tests should we unskip", "/heal-test-review".
metadata:
  heal-version: 0.3.2
  heal-source: bundled
---

# heal-test-review

Read-only skill that interprets the `[features.test]` findings
(`coverage_pct`, `change_coupling.drift`, `skip_ratio`) and
produces a prioritized TODO list grounded in the test pyramid,
the test-quality literature, and the user's actual codebase. The
mechanical write counterpart is `/heal-test-patch`.

## Mental model

`heal status --feature test --json` returns findings whose
`Finding.metric` is one of `coverage_pct`,
`change_coupling.drift`, or `skip_ratio`. Each metric measures a
different axis of test-quality decay; remediation depends on which
axis fired and why:

- **`coverage_pct`** — per-source-file line coverage from an
  externally-generated `lcov.info`. Findings only emit for `< 100%`
  files; severity comes from `[calibration.coverage_pct]`
  (≤ 5% Critical, > 75% Ok). Reads as: *the source ships untested
  code paths*.
- **`change_coupling.drift`** — a `TestSrc` pair where the test
  isn't co-evolving with its source (joint count below the
  project's `change_coupling.p50`). Severity::Medium. Reads as:
  *the test isn't being maintained alongside the code it claims to
  cover*.
- **`skip_ratio`** — per-test-file ratio of skipped to total tests,
  expressed as a percentage (> 1% Medium, > 5% High, > 20% Critical).
  Reads as: *the suite is accumulating dead weight*.

Findings carry a `Finding.is_test_file: bool` flag. When `true`, the
primary location is a test file (so the right remediation lives in
the test, not the source). When `false`, the finding is anchored on
production code (so the remediation is *adding* a test).

## Reading frame: the test pyramid, not Diátaxis

Tests have shape, and the shape carries cost-vs-confidence
trade-offs. Use the test pyramid (Fowler) — **many fast unit tests
at the base**, **fewer integration tests in the middle**, **a thin
end-to-end layer at the top** — as the reading lens. Crispin &
Gregory's quadrants and Beck's TDD discipline supplement it.

### Architectural patterns to surface

The shapes a `[features.test]` cache typically reveals:

- **Pyramid imbalance.** `coverage_pct` Critical concentrates on
  pure-logic modules (domain / application code) while integration
  tests dominate the suite. The pyramid is inverted — slow,
  brittle, expensive — and unit tests for the uncovered logic are
  the leverage move.
- **Hotspots without tests.** Findings where `hotspot=true` AND
  `coverage_pct` is Critical: the most-changed code is also the
  least-tested. T0 territory.
- **Skip accumulation.** A `skip_ratio` finding climbing past 5%
  signals the team is routing around failures rather than fixing
  them. Each skip needs its reason audited; some are fine
  (platform-specific, slow), most aren't.
- **Test/source coupling drift.** `change_coupling.drift` on a
  `TestSrc` pair where the test hasn't moved in N commits while the
  source has. The test is rotting in place; assertions probably no
  longer reflect the source's contract.

## When this skill is right

- Right after enabling `[features.test]` and running
  `/heal-test-reporter-setup`: the first set of test findings
  deserves an interpretation pass before mechanical fixes.
- After a major refactor: tests lag code, drift findings spike, and
  the user wants triage.
- Coverage targets came down from on high and the user wants to know
  *where* to add tests, not just that coverage is low.
- The user wants a TODO list to hand to a writer / next agent loop.

## Pre-flight

1. **Findings exist.** Run `heal status --feature test --json`. The
   command exits 1 with a stderr message when
   `[features.test].enabled = false` — bail and tell the user to
   enable the family (via `/heal-setup` or hand-edit
   `.heal/config.toml`) before retrying. On success the payload (or
   the cached `latest.json`) must contain at least one finding with
   a metric in `{coverage_pct, change_coupling.drift, skip_ratio,
   test_hotspot}`. If no `lcov.info` is reachable, recommend
   `/heal-test-reporter-setup` and stop.
2. **Capture the test config.** Open `.heal/config.toml` and read
   `[features.test]`. The `test_paths` globs decide which files
   carry `is_test_file=true`; the `coverage_pct` calibration band
   sets the lens through which to read severity.
3. **Worktree state noted.** When `worktree_clean` is false,
   mention it once: numbers reflect committed state plus
   uncommitted drift (and the lcov.info may be staler than the
   source).

## Procedure (Read → Investigate → Propose)

### Phase 1 — Read

For each `[features.test]` finding:

1. Note `metric`, `severity`, `hotspot`, `is_test_file`, primary
   location, and secondary locations. The `is_test_file` flag
   decides whether you're looking at the test side or the source
   side of the same problem.
2. Group by file. A source file at 12% coverage usually pairs with
   a thin / missing test file in `test_paths`; walk both together.
3. Open the file pair. The proposal needs to reflect what the test
   *should* assert after a fix, not just that coverage is low.

### Phase 2 — Investigate (test-pyramid lens)

Classify each uncovered source into one of three bands:

- **Pure logic (unit-test target).** No I/O, no clock, no global
  state — pure functions, value objects, data transforms. The
  remediation is unit tests; the cost is low; the leverage is
  highest. Most `coverage_pct` Critical findings on hotspots land
  here.
- **Coordination / orchestration (integration-test target).**
  Functions that compose other modules but contain little branching
  themselves. Unit-testing them with mocks usually proves nothing;
  prefer a small integration test that exercises the real
  collaborators.
- **I/O boundary (end-to-end target, sparingly).** Database calls,
  HTTP clients, filesystem walks. Tests here are slow and brittle;
  prefer a thin contract test plus heavy unit-testing inside the
  layer the boundary feeds.

Misclassification inflates suite cost: integration-testing pure
logic is wasteful, unit-testing an orchestrator with mocks is
ceremony. Match the test layer to the code's nature.

For `skip_ratio`, classify each skip by **why** it was added:

- **Platform / environment** (`@pytest.mark.skipif(sys.platform …)`)
  — usually fine; not actionable.
- **Slow / expensive** (`#[ignore]` on a 30-second integration test)
  — fine if the suite still runs it on demand; surface only when
  skipped indefinitely.
- **Broken** ("skip — flake to investigate", "skip — fails on CI")
  — the actionable category. The test is encoding "we know this is
  broken and we're routing around it". Surface for human decision.
- **Pending / TDD** (xtest, `pending`, write-the-test-first
  scaffolds) — surface as a TODO marker, not a skip to silence.

For `change_coupling.drift`, walk the source's recent commits
(`git log --since="6 months ago" -- <src>`) alongside the test's.
A drift pair where the source moved 30 commits and the test moved
zero is almost certainly stale.

### Phase 3 — Propose

Build a prioritized TODO list. Order matters — drain the
high-value, low-effort items first so the cache empties faster
under `/heal-test-patch`:

1. **Mechanical wins (allow-list).** Findings whose fix is
   obviously deterministic — adding a unit test for an uncovered
   hot path with documented behavior, aligning a drifted test
   whose source's contract is now clear, re-enabling a skipped
   test where the skip's reason no longer holds. Hand these to
   `/heal-test-patch`.
2. **Interpretive fixes.** Findings whose fix needs judgment —
   coverage gaps on functions whose intended behavior isn't
   documented anywhere, drift where the source has fundamentally
   changed (test asserts behavior the function no longer has),
   long-standing skips that may encode an architectural decision.
   The user (or another agent loop) drives these.
3. **Architectural changes.** Recurring drift on the same test
   pair, a coverage hole that maps to a module with no test
   neighbor, or a skip cluster on one suite signal the test
   architecture itself has shifted in a way no amount of patching
   will fix. Surface as separate architectural recommendations:
   pyramid rebalancing, test-suite split, retirement of an entire
   test file, or extracting a shared fixture.

## Drain target: Critical AND `hotspot=true`

The drain target stays **Critical AND `hotspot=true`** — same as
the rest of HEAL. *Not* "drive `skip_ratio` to zero", *not* "reach
100% coverage", *not* "delete every drifted test". Goodhart's Law
applies hard here: tests written to clear a coverage finding tend
to assert the type system rather than the behavior, and skips
removed under pressure tend to come back as flakes. The metric is
the proxy; the target is "the most-changed code keeps the
weakest safety net".

## Forbidden anti-patterns this skill must NOT recommend

- **Coverage-for-coverage's-sake tests.** Don't propose tests
  that exercise the type system, repeat the implementation
  (`assert add(2, 3) == 2 + 3`), or mock everything the function
  calls and assert the mocks. They raise the number, not the
  confidence.
- **Lowering coverage thresholds.** Nudging
  `[calibration.coverage_pct]` to make Critical disappear is
  Goodhart in cleartext.
- **Skipping tests to drain `skip_ratio`.** The metric measures
  real signal; right remediation is fix the test or document why
  it stays skipped.
- **Disabling `[features.test]` to clear the cache.** If the
  user has principled reasons to opt out, surface them
  explicitly; don't propose this as a workaround.
- **Re-enabling by weakening assertions.** "Unskip and swap
  `assert_eq!` for `assert!`" trades real signal for green CI.

## Output format

End with three blocks:

```
Architectural reading:
  - src/payments/engine.rs is Critical coverage_pct (12%) AND
    hotspot. Pure-logic module with branchy validation; this is
    the leverage move — write unit tests against documented
    contract, expect coverage to climb past 70% on three or four
    well-chosen cases.
  - Skip cluster in tests/integration/db/* (skip_ratio 18%, High):
    mostly "// skip until docker-compose is fixed" markers from
    six months ago. The suite has decided to route around the
    integration layer; surface as architectural — either fix the
    harness or formally retire the layer.
  - Two TestSrc drift pairs on src/cli/dispatch.rs ↔
    tests/cli_dispatch.rs: source moved 22 commits, test moved 0.
    Test asserts a CLI shape the binary no longer has.

Prioritized TODO:
  T1 Mechanical (hand to /heal-test-patch):
    - src/payments/engine.rs::validate_order  (coverage_pct, Crit🔥)
      contract is documented in module rustdoc; write 4 unit tests.
    - tests/cli_dispatch.rs::test_help_flag  (drift, Med)
      help text changed in 8a1b2c3; update the expected string.
    - tests/parser_test.rs::test_legacy_syntax  (skip_ratio, Med)
      skip reason "legacy syntax removed in v0.3" — delete the test.
  T2 Interpretive (user drives):
    - src/protocol/handshake.rs at 8% coverage: behavior isn't
      documented; writing a test would require guessing intent.
      Surface the documentation gap before testing.
    - tests/integration/db/* skip cluster: architectural call.
  T3 Architectural:
    - tests/integration/db/* (12 skipped, 0 active) — recommend
      retiring the layer or restoring the harness.
    - Pure-logic modules under src/domain/* sit at 18-30% coverage
      while tests/integration/* dominates LOC. Suite is inverted;
      consider seeding a tests/unit/ tree.

Counts:
  total findings:        24
  by metric:             coverage_pct=12 skip_ratio=4 drift=8
  by is_test_file:       source-side=12 test-side=12
  Critical AND hotspot:  3
```

## Constraints

- **Read-only.** Never edit a test, source, or `.heal/*` file from
  this skill.
- **Test-pyramid-aware.** Don't recommend integration coverage
  for pure logic, or unit coverage for orchestration. Match the
  test layer to the code's nature.
- **Drain target = Critical AND hotspot.** The hotspot flag is
  load-bearing here (R6: hotspot is decoration, not a target —
  but `Severity::Critical AND hotspot=true` is still the drain
  target across HEAL).
- **Don't moralize.** A test skipped 30 days is a signal, not a
  verdict. The user might have intentionally frozen the suite
  while a refactor lands.
- **English output.** Skill writes English; underlying tests may
  be in any language. When suggesting a rewrite of a non-English
  test name or comment, recommend the user (or a translator)
  handle it — don't auto-translate.
- **Defer to `/heal-test-reporter-setup`** when no `lcov.info` is
  produced. Without coverage data, two-thirds of the metrics in
  this family are unavailable.
