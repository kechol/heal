---
name: heal-test-reporter-setup
description: Detect the project's language stack (Rust / Python / JS-TS / Go / Scala or mixed) and propose the lcov reporter configuration plus CI integration so `lcov.info` lands at one of HEAL's default `lcov_paths`. Read-only on the codebase; proposes commands and config edits without running them. Trigger on "set up coverage reporting", "configure lcov for heal", "wire up coverage", "/heal-test-reporter-setup".
---

# heal-test-reporter-setup

One-shot skill that prepares a project to feed coverage data into
HEAL's `[features.test]` observer family. It works in two phases:
**detect** which language ecosystem the project lives in, and
**propose** the reporter config + CI integration that ends with an
`lcov.info` file HEAL can read.

The HEAL binary is a deterministic *consumer* of `lcov.info`; it has
no test runner, no instrumentation hooks, no in-process coverage
collection. That's why generation lives here, in a user-triggered
skill, rather than inside `heal status`.

Read-only on source files. The skill **does not run any commands**;
it proposes the exact shell invocations and config snippets the user
runs themselves. The `[features.test.coverage]` consumer under
`heal status` and `heal metrics` reads `lcov.info` the next time it
runs.

## When this skill is right

- First-time setup right after enabling `[features.test]` in
  `.heal/config.toml`: HEAL emits no `coverage_pct` findings until an
  `lcov.info` is reachable.
- The project switched test runners (jest → vitest, unittest →
  pytest) and the previous reporter config no longer applies.
- The user added a new language to a polyglot repo (e.g. a Rust
  workspace gained a Python pipeline) and wants coverage for both.

## Mental model

The HEAL observer reads `lcov.info` from one of the configured
`lcov_paths`. Default search order (set in `[features.test.coverage]`):

```toml
lcov_paths = [
  "lcov.info",
  "coverage/lcov.info",
  "target/llvm-cov/lcov.info",
  "coverage/lcov-report/lcov.info",
]
```

The skill's job is to make sure **at least one of these paths**
contains a current `lcov.info` after the project's test run. The
right reporter is the one whose default output matches; when it
doesn't, point `lcov_paths` at the right place rather than fighting
the tool.

## Pre-flight

1. **`[features.test]` enabled.** Check `.heal/config.toml`. If
   `[features.test] enabled = false` (or the section is absent),
   tell the user the reporter would have no consumer and ask
   whether to enable the feature. If they decline, stop — wiring up
   coverage without the consumer is busy-work.
2. **`[features.test.coverage].enabled` state.** When
   `[features.test.coverage] enabled = false`, propose flipping it
   to `true` as part of the output (the user pastes the edit
   themselves).
3. **Detection plan.** The detection step walks the repo root for
   ecosystem markers; do that walk before proposing anything.

## Phase 1 — Detect the stack

Walk the repo root once and check for these markers, in order. A
polyglot repo can match multiple — propose for each in turn.

### Rust — `Cargo.toml` present

Default reporter: `cargo-llvm-cov`, lands at `lcov.info` by default.

Propose:

```sh
cargo install cargo-llvm-cov
cargo llvm-cov --workspace --lcov --output-path lcov.info
```

For a single-crate repo, drop `--workspace`. For monorepos with
mixed test runners, run from the workspace root.

`cargo-llvm-cov` matches the `lcov.info` default path. No
`lcov_paths` edit needed.

### Python — `pyproject.toml`, `setup.py`, or `requirements.txt`

Default reporter: `pytest-cov`. Detect the test runner inside
`pyproject.toml` (`[tool.pytest.ini_options]`) or
`tox.ini` / `setup.cfg`. Propose:

```sh
pip install pytest pytest-cov
pytest --cov=<src-package> --cov-report=lcov:coverage/lcov.info
```

Replace `<src-package>` with the project's source root (e.g.
`--cov=src` or `--cov=heal_pipeline`). The output path
`coverage/lcov.info` matches the second default `lcov_paths` entry.

For projects on `unittest` (no pytest), suggest migrating the
coverage entrypoint to `coverage.py` directly:

```sh
coverage run -m unittest discover
coverage lcov -o coverage/lcov.info
```

### JavaScript / TypeScript — `package.json` present

Detect the runner from `package.json` `devDependencies`:

- **jest** (`"jest"` in deps): propose

  ```sh
  jest --coverage --coverageReporters=lcov
  ```

  Jest writes `coverage/lcov.info` by default — matches the second
  default `lcov_paths` entry.

- **vitest** (`"vitest"` in deps): propose adding to
  `vitest.config.ts`:

  ```ts
  import { defineConfig } from "vitest/config";

  export default defineConfig({
    test: {
      coverage: {
        provider: "v8",
        reporter: ["lcov"],
        reportsDirectory: "coverage",
      },
    },
  });
  ```

  Then `npx vitest run --coverage`. Output: `coverage/lcov.info`.

- **mocha + nyc**: propose

  ```sh
  npx nyc --reporter=lcov mocha
  ```

  Output: `coverage/lcov.info`.

If the project pins a runner that isn't one of those, surface it in
the output and ask the user which they want — don't guess.

### Go — `go.mod` present

Go's built-in coverage emits a `coverage.out` file (its own format,
not lcov). Convert with `gcov2lcov`:

```sh
go install github.com/jandelgado/gcov2lcov@latest
go test -coverprofile=coverage.out ./...
gcov2lcov -infile=coverage.out -outfile=coverage/lcov.info
```

Output: `coverage/lcov.info`. Matches default `lcov_paths`.

### Scala — `build.sbt` present

Use the `scoverage` plugin:

1. Add to `project/plugins.sbt`:

   ```scala
   addSbtPlugin("org.scoverage" % "sbt-scoverage" % "2.0.12")
   ```

2. Run:

   ```sh
   sbt clean coverage test coverageReport
   ```

The plugin writes lcov to
`target/scala-<version>/scoverage-report/scoverage.xml` plus an lcov
output the user must point HEAL at. Propose updating `lcov_paths`:

```toml
[features.test.coverage]
lcov_paths = [
  "lcov.info",
  "coverage/lcov.info",
  "target/llvm-cov/lcov.info",
  "coverage/lcov-report/lcov.info",
  "target/scala-2.13/scoverage-report/lcov.info",  # added
]
```

### Polyglot — multiple markers present

Propose each language's setup separately. The user can run them in
parallel (CI) or sequentially. The default `lcov_paths` covers the
top three slots; languages whose default output sits elsewhere (e.g.
Scala) need an explicit `lcov_paths` extension.

The HEAL observer reads **all** matching paths and merges by source
file. For overlapping per-file coverage (rare — a Rust file covered
by both `cargo-llvm-cov` and a different tool), the highest line
hit count wins.

## Phase 2 — Write step (proposed, not run)

Output a single block the user pastes into their terminal / editor.
Two parts:

### Part A — `.heal/config.toml` edits

Show the diff against the user's current config. Common shape:

```toml
# .heal/config.toml

[features.test]
enabled = true

[features.test.coverage]
enabled = true
# lcov_paths defaults cover the top reporters; uncomment the line
# below only if your reporter writes elsewhere.
# lcov_paths = ["custom/path/lcov.info", ...]
```

### Part B — local invocation

The exact command(s) to produce the lcov file. Single-language
projects get one block; polyglot projects get one block per
language with a comment naming each.

### Part C — verification handshake

Tell the user to run:

```sh
<reporter command>            # produces lcov.info
heal status --refresh --json  # HEAL re-scans and picks up coverage
```

After that, `heal status` should emit `coverage_pct` findings. If
not, the path doesn't match `lcov_paths` — surface that and propose
the `lcov_paths` edit.

## Phase 3 — CI integration suggestion

Coverage in CI keeps the `latest.json` everyone shares accurate. Per
common CI providers:

### GitHub Actions

Append to the test job (or its own job depending on runtime cost):

```yaml
- name: Generate coverage
  run: cargo llvm-cov --workspace --lcov --output-path lcov.info
- name: Upload coverage artifact
  uses: actions/upload-artifact@v4
  with:
    name: lcov.info
    path: lcov.info
```

Replace the `Generate coverage` step's command with the appropriate
one for the detected stack (`pytest --cov ...`, `jest --coverage ...`,
`go test -coverprofile=... && gcov2lcov ...`, `sbt clean coverage
test coverageReport`).

If the team commits the lcov file (uncommon, but supported), suggest
the post-commit hook will pick it up automatically.

### GitLab CI

```yaml
test:
  script:
    - <reporter command>
  artifacts:
    paths:
      - lcov.info
    expire_in: 1 week
```

### CircleCI

```yaml
- run: <reporter command>
- store_artifacts:
    path: lcov.info
```

For all providers, the artifact is so a downstream HEAL job (or a
maintainer pulling the artifact locally) can run
`heal status --refresh` against the same `lcov.info` and see the
same findings as anyone else on the same commit.

## Output format

Single block, sectioned, with copy-paste commands clearly delimited.
Example for a Rust workspace:

```
Detected: Rust workspace (Cargo.toml at root + 3 member crates)

1. Install reporter (one-time):
   cargo install cargo-llvm-cov

2. Enable [features.test.coverage] in .heal/config.toml:
   [features.test]
   enabled = true

   [features.test.coverage]
   enabled = true

3. Run locally:
   cargo llvm-cov --workspace --lcov --output-path lcov.info
   heal status --refresh --json

4. Wire into CI (.github/workflows/ci.yml):
   - name: Generate coverage
     run: cargo llvm-cov --workspace --lcov --output-path lcov.info
   - uses: actions/upload-artifact@v4
     with:
       name: lcov.info
       path: lcov.info

5. lcov_paths: defaults already cover lcov.info — no edit needed.

After the first run, heal status will emit coverage_pct findings
for source files below the calibration threshold (≤ 5% Critical,
> 75% Ok). To review them, run /heal-test-review. To drain them
mechanically, run /heal-test-patch.
```

## Constraints

- **Don't run any commands.** This skill is a configuration
  proposer, not an installer or test runner. The user runs every
  command in their terminal so they see the output and can inspect
  changes.
- **Don't auto-edit `.heal/config.toml`.** Surface the proposed
  edits as a copy-pasteable block; let the user apply them
  themselves. Only `/heal-config` writes that file.
- **Don't auto-edit CI configs.** Same reason — pasting into a CI
  workflow is a deploy decision.
- **Match the default `lcov_paths` when possible.** If the
  reporter's default output already lands in one of the four
  default paths, don't propose an `lcov_paths` edit. Only extend
  the list when the tool's output sits elsewhere (Scala / scoverage,
  custom output dirs).
- **Don't recommend coverage thresholds.** The calibration band
  comes from `[calibration.coverage_pct]`, not from this skill.
  Wiring up the reporter is independent of where Critical / High
  cut off.
- **Polyglot is supported.** When the repo has multiple language
  markers, propose each setup in turn; don't pick one and ignore
  the others.
- **English output.** Skill writes English; the user's CI / config
  files may be in any language.
