---
title: Test · Configuration
description: How to enable [features.test], wire up an lcov.info, and tune which files heal treats as tests.
---

The **Test** family is opt-in. Off by default — turn it on once
your project has (or is willing to wire up) an `lcov.info` produced
by `cargo llvm-cov`, `pytest --cov`, `nyc`, or `scoverage`. heal
never executes tests itself; everything that needs the test suite
to actually run (flakiness, mutation score, runtime trends) stays
out of scope.

For what each metric flags, see [Test › Metrics](/heal/test/metrics/).
For the bundled skills, see [Test › Skills](/heal/test/skills/).

## Quick enable

```toml
[features.test]
enabled = true

[features.test.coverage]
enabled = true
```

Defaults cover Rust / TypeScript / JavaScript / Python / Go / Scala
test conventions and the four conventional `lcov.info` paths. Most
projects don't need to override anything.

If you don't have an `lcov.info` yet, run the bundled setup
skill — it inspects your stack and proposes the reporter wiring.

```sh
claude /heal-test-reporter-setup
```

For the full skill contract see
[Test › Skills](/heal/test/skills/#heal-test-reporter-setup-—-wire-up-lcov).

## `[features.test]`

```toml
[features.test]
enabled    = false                # master switch
test_paths = [
  "tests/**",
  "**/*_test.rs",
  "**/*.test.ts", "**/*.test.tsx", "**/*.test.js", "**/*.test.jsx",
  "**/*.spec.ts", "**/*.spec.tsx", "**/*.spec.js", "**/*.spec.jsx",
  "**/__tests__/**",
  "**/*_test.go",
  "**/test_*.py", "**/*_test.py",
  "**/*Test.scala", "**/*Spec.scala",
]
```

- `enabled` (default `false`) — master switch. While false, every
  test observer is a no-op.
- `test_paths` (default: language conventions above) — gitignore-
  syntax globs that mark which source files are tests. The
  `skip_ratio` observer walks these files; every Finding whose
  primary file matches is also tagged `is_test_file = true`.

When `test_paths` is empty, heal falls back to a built-in
heuristic covering the same conventions.

### `is_test_file` flag

When `[features.test]` is enabled, every Finding gains an
`is_test_file: bool` flag. Skills filter on this to read test- and
production-side severities independently — `/heal-test-review`
focuses on test findings; `/heal-code-review` focuses on
production findings.

The flag is omitted from JSON output when false, so projects that
don't enable the test family see byte-identical `latest.json`
content to before.

## `[features.test.coverage]`

```toml
[features.test.coverage]
enabled    = false
lcov_paths = [
  "lcov.info",
  "coverage/lcov.info",
  "target/llvm-cov/lcov.info",
  "coverage/lcov-report/lcov.info",
]
```

- `enabled` (default `false`) — sub-feature switch. Keep
  `[features.test]` on but `[features.test.coverage]` off when you
  want `is_test_file` tagging and `skip_ratio` without yet wiring
  up a reporter.
- `lcov_paths` — project-relative paths probed in order. **First
  existing file wins**; the rest are ignored. Missing files are
  silent — no warning at startup.
- `post_commit_refresh` (default unset) — optional shell command
  the post-commit hook spawns in the background to re-run your
  reporter after every commit. The process is detached and its
  output discarded so your commit flow doesn't wait. Pair it with
  the same command `/heal-test-reporter-setup` proposes
  (`cargo llvm-cov --workspace --lcov --output-path lcov.info
  --locked --ignore-run-fail`, `pytest --cov=...`, etc.) so the
  next `heal status` reads fresh `lcov.info`. Skipped silently
  when `[features.test]` or `[features.test.coverage]` is off.

heal reads what your CI / local reporter produces. The default
probe order covers:

| Reporter | Path written |
|---|---|
| `cargo llvm-cov --lcov` | `target/llvm-cov/lcov.info` |
| `pytest --cov --cov-report=lcov` | `coverage/lcov.info` |
| `nyc --reporter=lcov` | `coverage/lcov-report/lcov.info` |
| `scoverage` (Scala) | varies; symlink to `lcov.info` if needed |

The lcov reader is permissive — it tolerates unknown record types
and recovers totals from per-line records when reporters omit the
summary fields, so most reporter dialects work out of the box.

## Calibration

Two new sections appear in `.heal/calibration.toml` when you run
`heal calibrate --force` with the test family on:

```toml
[calibration.coverage_pct]
# Heal stores INVERTED values (100 - coverage_pct) so the same
# `value >= p95 → Critical` cascade applies as for the rest of the
# metrics — worst still maps to Critical.
p50 = 30.0     # 70% coverage
p75 = 50.0     # 50% coverage
p90 = 70.0     # 30% coverage
p95 = 85.0     # 15% coverage
floor_critical = 95.0   # ≤ 5% coverage → Critical regardless of percentile
floor_ok       = 25.0   # > 75% coverage → Ok regardless of percentile

[calibration.skip_ratio]
p50 = 0.0
p75 = 1.0
p90 = 5.0
p95 = 10.0
floor_critical = 20.0   # > 20% skip → Critical
floor_ok       = 0.5    # < 0.5% skip → Ok
```

These are the literature-anchored fallbacks heal uses until you
run `heal calibrate --force`. Floors belong in `config.toml`, not
here, so they survive recalibration:

```toml
[metrics.coverage_pct]
floor_critical = 90.0   # tightens to "≤ 10% coverage → Critical"

[metrics.skip_ratio]
floor_ok = 0.0          # any skipped test surfaces
```

(`coverage_pct` overrides apply to the inverted form —
`floor_critical = 90.0` means "≤ 10% line coverage", not "≤ 90%".)

## Post-commit nudge

When `[features.test.coverage]` is on, the post-commit hook adds an
indented second line to the nudge:

```
heal: recorded · 3 critical, 7 high · heal status
         · 2 uncovered hotspot
```

The count is the number of `coverage_pct` findings at High or
Critical severity that also carry `hotspot=true`. The line is
suppressed when the coverage feature is off.

## Strict by design

Like every other section, `[features.test]` and
`[features.test.coverage]` reject unknown keys:

```toml
[features.test]
test_path = ["tests/**"]   # ✘ unknown — heal errors here
                            #   (it's `test_paths`, plural)
```
