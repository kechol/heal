---
title: Test · Metrics
description: The three test-quality metrics the [features.test] family adds — coverage_pct, skip_ratio, and Test Hotspot — plus the change_coupling.drift submetric.
---

The opt-in **Test** family adds three top-level metrics on top of
the always-on Code family — `coverage_pct`, `skip_ratio`, and Test
Hotspot — plus the `change_coupling.drift` submetric on
`change_coupling`. The headline signal is **line coverage**, read
from an externally generated `lcov.info` and fed back into Hotspot
scoring so uncovered hot paths bubble to the top of the queue.

For configuration knobs see [Test › Configuration](/heal/test/configuration/).
For the bundled skills see [Test › Skills](/heal/test/skills/).

## At a glance

| Metric | Layer | What it flags |
|---|---|---|
| `coverage_pct` | per-source-file | line coverage parsed from `lcov.info`; only files with `< 100%` produce findings |
| `skip_ratio` | per-test-file | skipped tests as a percentage of total tests in the file |
| `test_hotspot` | per-source-file | `commits × uncov_pct` composite — flips `hotspot=true` on `coverage_pct` Findings |
| `change_coupling.drift` | per-pair (submetric) | a test paired with a source that has been changing without it |

Plus a structural addition: every Finding gains an
`is_test_file: bool` flag so skills can read test- and
production-side severities independently.

## `coverage_pct`

> _"Which production code is dark to the test suite?"_

Per-source-file line coverage parsed from the first existing
`lcov.info` in `[features.test.coverage].lcov_paths`. Findings are
emitted only for files with `< 100%` coverage. Calibration stores
**inverted values** (`100 - coverage_pct`) so the same "value
reaches p95 → Critical" cascade applies as for the rest of the
metrics — see
[Test › Configuration](/heal/test/configuration/#calibrationseverity-基準の調整)
for the floors.

## `skip_ratio`

> _"Which test files carry a meaningful percentage of skipped
> tests?"_

Per-test-file ratio of skipped tests to total tests. heal walks
files matched by `[features.test].test_paths` and counts
language-specific skip markers — `#[ignore]` (Rust),
`@pytest.mark.skip` / `@unittest.skipIf` (Python), `it.skip` /
`xit` / `xdescribe` (JS / TS), `t.Skip()` (Go), ScalaTest
`ignore` / `pending`. Detection is structural; comments and string
literals can't trigger false positives.

## `change_coupling.drift`

> _"Which tests aren't keeping up with the source they cover?"_

When `[features.test]` is on, test ↔ source pairs whose joint
co-change count sits **below** the project's median (the test
hasn't been moving with the source) get re-tagged from
`change_coupling.expected` (Advisory) to `change_coupling.drift`
(Medium). Read it as: "the test exists, but every recent change
to the source is happening without it".

Doc ↔ source pairs never promote to drift — drift is a
test-quality signal.

## Test Hotspot — where the code keeps changing without tests

Test Hotspot is the test-family analogue of code Hotspot. It
ranks src files by `commits × uncov_pct`: high score = the file
keeps changing **and** large slices of it stay untested. A low-CCN
config loader with 0% coverage and 30 commits is a real test
target that code Hotspot would miss.

Files that lcov never mentioned but git churn touched count as
100% gap (= untested). Files at 100% coverage drop to score 0.

Test Hotspot itself always carries `Severity::Ok`; its job is to
flip `hotspot=true` on `coverage_pct` Findings on the same file —
so the drain target stays "Critical AND `hotspot=true`", just
scoped to the test family now.

## Post-commit nudge: "uncovered hotspot"

```
heal: recorded · 3 critical, 7 high · heal status
         · 2 uncovered hotspot
```

The count is the number of `coverage_pct` findings at High or
Critical Severity that also carry `hotspot=true` — the shortest
possible "the next test should land here" reminder. Suppressed
when `[features.test.coverage]` is off or no High / Critical
`coverage_pct` sits on a hotspot.

## Drain pattern

`/heal-test-review` frames the findings through the test-pyramid
lens (unit / integration / e2e). `/heal-test-patch` works through
them one commit at a time: write the missing unit test for
`coverage_pct`, re-enable a justified skip for `skip_ratio`, align
the drifted test for `change_coupling.drift`. The patch skill
refuses to weaken assertions or paper over real flakes — see
[Test › Skills](/heal/test/skills/) for the full contract.
