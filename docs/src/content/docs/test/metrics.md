---
title: Test · Metrics
description: The three test-quality metrics the [features.test] family produces — coverage_pct, skip_ratio, change_coupling.drift — plus how they shape Hotspot scoring.
---

The opt-in **Test** family adds three metrics on top of the
always-on Code family. The headline signal is **line coverage** —
read from an externally generated `lcov.info` and fed back into
hotspot scoring so uncovered hot paths bubble to the top of the
drain queue.

For configuration knobs see [Test › Configuration](/heal/test/configuration/).
For the bundled skills see [Test › Skills](/heal/test/skills/).

## At a glance

| Metric | Layer | What it flags |
|---|---|---|
| `coverage_pct` | per-source-file | line coverage parsed from `lcov.info`; only files with `< 100%` produce findings |
| `skip_ratio` | per-test-file | skipped tests as a percentage of total tests in the file |
| `change_coupling.drift` | per-pair | a test paired with a source that has been changing without it |

Plus a structural addition: every Finding gains an
`is_test_file: bool` flag so skills can read test- and
production-side severities independently.

## `coverage_pct`

> _"Which production code is dark to the test suite?"_

Per-source-file line coverage parsed from the first existing
`lcov.info` in `[features.test.coverage].lcov_paths`. The reader
handles `cargo llvm-cov`, `pytest --cov`, `nyc`, and `scoverage`
dialects — including reporters that emit duplicate file records
(merged by max-of, not summed, so overlapping coverage isn't
double-counted).

Findings are emitted only for files with `< 100%` coverage. Fully
covered files don't produce noise findings.

### How severity is decided

Calibration stores **inverted values** (`100 - coverage_pct`) so
the same "value reaches p95 → Critical" cascade applies as for the
rest of the metrics. Until you run `heal calibrate --force`, a
literature-anchored fallback is used:

| Coverage | Severity (default) |
|---|---|
| ≤ 5%   | Critical |
| ≤ 15%  | Critical (via p95) |
| ≤ 30%  | High (via p90) |
| ≤ 50%  | Medium (via p75) |
| > 75%  | Ok (via floor) |

Override the floors in `config.toml` — see
[Test › Configuration](/heal/test/configuration/#calibration).

### What's out of scope

heal never executes tests. **Forever** out of scope: flakiness,
runtime trends, isolation, mutation score — anything that needs
the test suite to actually run. CI is the right home for those
signals; heal stays read-only on the lcov artifact.

## `skip_ratio`

> _"Which test files have a meaningful percentage of skipped
> tests?"_

Per-test-file ratio of skipped tests to total tests. heal walks
files matched by `[features.test].test_paths` and counts
language-specific skip markers — `#[ignore]` (Rust),
`@pytest.mark.skip` / `@unittest.skipIf` (Python), `it.skip` /
`xit` / `xdescribe` (JS / TS), `t.Skip()` (Go), ScalaTest
`ignore` / `pending` — over the total test count.

Detection is structural: comments and string literals can't
trigger false positives.

### How severity is decided

Calibrated against `[calibration.skip_ratio]`. Until you run
`heal calibrate --force`, the fallback is:

| Skip rate | Severity (default) |
|---|---|
| < 0.5% | Ok |
| > 1%   | Medium |
| > 5%   | High |
| > 10%  | Critical |
| > 20%  | Critical (via floor) |

Findings are emitted only for files with at least one skipped
test.

## `change_coupling.drift`

> _"Which tests aren't keeping up with the source they cover?"_

When `[features.test]` is on, test ↔ source pairs that should be
moving together but aren't are surfaced as a real Finding rather
than being filed under Advisory.

A test ↔ source pair whose joint co-change count sits **below** the
project's median (`change_coupling.p50`) is re-tagged from
`change_coupling.expected` (Advisory) to `change_coupling.drift`
(Medium). Read it as: "the test exists, but every recent change to
the source is happening without it".

Doc ↔ source pairs never promote to drift — drift is a test-quality
signal.

## `test_hotspot` — where unverified change is piling up

`test_hotspot` is the test-family analogue of code Hotspot. It
ranks src files by `commits × uncov_pct`: high score = the file
keeps changing **and** large slices of it stay untested. The
literature anchor for "unverified change" is more direct than CCN
for the test question — a low-CCN config loader with 0% coverage
and 30 commits is a real test target that code Hotspot would
miss.

```
score = commits_in_90d × (100 - line_coverage_pct)
```

Files that lcov never mentioned but git churn touched count as
100% gap (= untested) — the case the metric exists to surface.
Files at 100% coverage drop to score 0.

`test_hotspot` is itself always `Severity::Ok`; the score's job
is to flip `hotspot=true` on `coverage_pct` Findings on the same
file. Drain target stays "Critical AND `hotspot=true`", just
scoped to the test family now.

Default graduation gate is `[features.test.hotspot] floor_ok = 25`
(roughly "1 commit × 25% gap" — the literature anchor for
"75% coverage = decent" gives the gap floor). Override if the
project's `test_hotspot` distribution makes the default too
strict / too loose.

## Post-commit nudge: "uncovered hotspot"

```
heal: recorded · 3 critical, 7 high · heal status
         · 2 uncovered hotspot
```

The count is the number of `coverage_pct` findings at High or
Critical severity that also carry `hotspot=true` — the shortest
possible "the next test should land here" reminder.

The line is suppressed when `[features.test.coverage]` is off, or
when no High / Critical `coverage_pct` finding sits on a hotspot.

## How `/heal-test-review` and `/heal-test-patch` use these

`/heal-test-review` reads `heal status --json`, filters to the test
family, and frames the findings through the test-pyramid lens
(unit / integration / e2e).

`/heal-test-patch` drains the test slice of the cache, one finding
per commit:

- **`coverage_pct`** → write or extend a unit test for an uncovered
  hot path.
- **`skip_ratio`** → re-enable a skipped test whose reason no longer
  holds, or document why it stays skipped.
- **`change_coupling.drift`** → align the drifted test with its
  source. The patch skill surfaces both files together.

Refusals encoded in the patch skill: assertion-weakening,
skip-the-flake, scaffold-without-running. See
[Test › Skills](/heal/test/skills/) for the full contract.
