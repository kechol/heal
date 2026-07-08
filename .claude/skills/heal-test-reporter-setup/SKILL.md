---
name: heal-test-reporter-setup
description: Detect the project's language stack (Rust / Python / JS-TS / Go / Scala or mixed), then with per-step `AskUserQuestion` approval install the lcov reporter, flip `[features.test.coverage].enabled` in `.heal/config.toml`, run the reporter, optionally wire `[features.test.coverage].post_commit_refresh` so HEAL's post-commit hook re-runs the reporter in the background, and verify HEAL picks up the resulting `lcov.info`. Edits `.heal/config.toml` and runs install / reporter commands; CI workflow edits stay a copy-pasteable proposal. Trigger on "set up coverage reporting", "configure lcov for heal", "wire up coverage", "refresh lcov on every commit", "/heal-test-reporter-setup".
metadata:
  heal-version: 0.4.0
  heal-source: bundled
---

# heal-test-reporter-setup

Wires a project up to feed coverage data into HEAL's
`[features.test]` observer family. Three phases: **detect** the
language ecosystem, **install + configure + run** the reporter
under per-step approval (no install, no config edit, no reporter
run lands until the user approves it), then **verify**
`heal status` picks up the resulting `lcov.info`.

The skill performs side effects: `cargo install`, `pip install`,
`npm install`, `go install`, `sbt`, edits to
`.heal/config.toml`, and the reporter run that produces
`lcov.info`. CI workflow edits remain a **proposal** — CI changes
affect every contributor's PR, so they get printed as a
copy-pasteable block and never auto-applied.

## When this skill is right

- First-time setup right after enabling `[features.test]` in
  `.heal/config.toml`: HEAL emits no `coverage_pct` findings until
  an `lcov.info` is reachable.
- Switched test runner (jest → vitest, unittest → pytest) and the
  previous reporter no longer applies.
- Polyglot repo gained a new language and wants coverage for it.

## Output language

Write the per-step approval prompts, install commentary, and final
verification report in the user's language. Resolution order:

1. Explicit instruction in the current conversation.
2. The language the user is writing in (the chat conversation
   language exposed by the host agent — Claude Code, Codex CLI, …).
3. `[project].response_language` in `.heal/config.toml` (free-form:
   `"Japanese"`, `"日本語"`, `"ja"`, `"français"`).
4. English (fallback).

Identifiers stay verbatim — install commands (`cargo install
cargo-llvm-cov`, `pip install coverage`), config keys
(`[features.test.coverage]`, `lcov_paths`), file paths (`lcov.info`,
`coverage/lcov.info`), and the copy-pasteable CI workflow snippet
are part of the contract. Translate the surrounding explanation,
not the commands.

## Mental model

The HEAL observer reads `lcov.info` from one of the configured
`lcov_paths`. Default search order (set in
`[features.test.coverage]`):

```toml
lcov_paths = [
  "lcov.info",
  "coverage/lcov.info",
  "target/llvm-cov/lcov.info",
  "coverage/lcov-report/lcov.info",
]
```

The skill makes sure **at least one of these paths** holds a
current `lcov.info` after the project's test run. The right
reporter is the one whose default output matches; when it
doesn't, extend `lcov_paths` rather than fight the tool.

## Pre-flight

1. **`[features.test]` enabled.** Read `.heal/config.toml`. If
   disabled (or absent), ask whether to flip it on. Stop if
   declined — the rest of the skill has no consumer.
2. **Existing reporter check.** If `lcov.info` already exists at
   one of `lcov_paths` and is recent (mtime within 24 h), report
   it and ask whether to skip the install / run step (idempotent
   re-run).
3. **Detection plan.** Walk the repo root for ecosystem markers
   before asking anything; show the detected stack in the first
   `AskUserQuestion`.

## Phase 1 — Detect the stack

Walk the repo root once. Markers, in order. A polyglot repo can
match several — in Phase 2 each ecosystem gets its own
confirm-and-execute sequence.

### Rust — `Cargo.toml` present

Reporter: `cargo-llvm-cov`. Default output: `lcov.info`.
Component: `llvm-tools-preview` (rustup), needed for
`llvm-profdata` + `llvm-cov` (the binaries that convert profraw →
lcov). cargo-llvm-cov 0.5+ auto-installs the component on first
run; pinning it in CI via
`dtolnay/rust-toolchain@... with: components: llvm-tools-preview`
keeps job time predictable.

Workspace flag: include `--workspace` for multi-crate workspaces;
drop it for single-crate repos. **Use `--ignore-run-fail`** so a
single test that's flaky under instrumentation (env-var leakage
to spawned subprocesses, timing-sensitive code) doesn't block the
whole lcov emit. The flag still surfaces failed test names so the
user can investigate separately.

### Python — `pyproject.toml`, `setup.py`, or `requirements.txt`

Reporter: `pytest-cov`. Detect the test runner inside
`pyproject.toml` (`[tool.pytest.ini_options]`) or
`tox.ini` / `setup.cfg`. Output: `coverage/lcov.info` (matches
default `lcov_paths`).

For `unittest`-only projects (no pytest), use `coverage.py`
directly:
`coverage run -m unittest discover && coverage lcov -o coverage/lcov.info`.

### JavaScript / TypeScript — `package.json` present

Detect the runner from `devDependencies`:

- **jest**: `jest --coverage --coverageReporters=lcov`. Output:
  `coverage/lcov.info`.
- **vitest**: needs `vitest.config.ts` with the v8 / istanbul
  provider and `reporter: ["lcov"]`. Output:
  `coverage/lcov.info`.
- **mocha + nyc**: `npx nyc --reporter=lcov mocha`. Output:
  `coverage/lcov.info`.

If the project pins a runner that isn't one of those, surface it
and ask the user which they want — don't guess.

### Go — `go.mod` present

Go's built-in coverage emits `coverage.out` (its own format). Convert with `gcov2lcov`:
`go test -coverprofile=coverage.out ./... && gcov2lcov -infile=coverage.out -outfile=coverage/lcov.info`.

### Scala — `build.sbt` present

Plugin: `scoverage`. Output:
`target/scala-<v>/scoverage-report/lcov.info` — **not** in
default `lcov_paths`, so this is the one ecosystem where the
skill always proposes a `lcov_paths` extension in Phase 2 Step b.

### Polyglot

Each detected ecosystem runs through Phase 2 separately. The user
can opt out of any one ecosystem at the per-step prompt.

## Phase 2 — Confirm and Execute

For each detected ecosystem, run the four-step sequence below.
Every step gates on `AskUserQuestion`. **Default to skip** when
the user declines or doesn't reply — never auto-install.

### Step a — Install the reporter

`AskUserQuestion`:

> Install `<reporter>` for `<lang>`?
>
> - **Install** (Recommended): runs `<exact command>`. ~<duration>.
> - **Skip**: leave the reporter to the user; the skill stops here.

On Install: shell out via `Bash`. The first install is slow
(cargo-llvm-cov: ~1–2 min compile; pytest-cov: ~10 s; jest: ~20 s
fresh; sbt-scoverage: ~30 s). Use `run_in_background: true` and
wait for completion. Print only the final 5–10 lines on success;
print full stderr on failure and stop.

On Skip: stop the skill. Re-invoking later resumes from this step
(idempotent — Phase 1 detects the install state).

When the user has a tool-version manifest (`mise.toml`,
`.tool-versions`, `asdf`, `pyenv`-managed `requirements*.txt`),
prefer pinning there over a bare global install. For mise:

```toml
# mise.toml
[tools]
"cargo:cargo-llvm-cov" = "0.8.5"
```

Surface this as a follow-up in the run summary; don't second-guess
the user's choice if they declined the manifest edit.

### Step b — Edit `.heal/config.toml`

Show the diff first (current `[features.test.coverage]` block vs
proposed). Then `AskUserQuestion`:

> Apply this edit to `.heal/config.toml`?
>
> - **Apply** (Recommended): write the change.
> - **Skip**: keep the current config; the skill stops here.

On Apply: `Edit` the TOML in place. Then run
`heal status --refresh --feature test --json` once to confirm the
file parses (`Config::from_toml_str` uses `deny_unknown_fields`,
so a typo surfaces immediately). On parse failure, surface the
error and revert the edit.

When the reporter's default output is **not** in default
`lcov_paths` (Scala / scoverage; custom output dirs), extend
`lcov_paths` in the same edit. When the existing file already has
`enabled = true`, mention it and skip.

### Step c — Run the reporter

`AskUserQuestion`:

> Run `<reporter command>` now? Time: ~<estimate>.
>
> - **Run** (Recommended): shells out and waits.
> - **Skip**: skip the run; the user runs it themselves later.

On Run: `Bash` with `run_in_background: true` (these runs are
slow — Rust workspace coverage is several minutes; cargo-llvm-cov
recompiles with instrumentation). Use the language-specific
flags from Phase 1:

- Rust: `cargo llvm-cov --workspace --lcov --output-path lcov.info --locked --ignore-run-fail`
- Python: `pytest --cov=<src-package> --cov-report=lcov:coverage/lcov.info`
- jest: `jest --coverage --coverageReporters=lcov`
- vitest: `npx vitest run --coverage`
- Go: `go test -coverprofile=coverage.out ./... && gcov2lcov -infile=coverage.out -outfile=coverage/lcov.info`
- Scala: `sbt clean coverage test coverageReport`

Print failing-test names if the run reports any (under
`--ignore-run-fail` they are non-fatal but still informative).

### Step d — Verify

After the reporter run, run:

```sh
heal status --refresh --feature test --json
```

Assert at least one `coverage_pct` Finding is present. If not:

- Confirm `lcov.info` exists at one of `lcov_paths` (use
  `wc -l <path>` to confirm non-empty).
- Print the actual path and `lcov_paths` value so the user can
  reconcile.
- Stop. Don't auto-recover.

### Step e — Wire post-commit refresh (optional)

Coverage data goes stale fast — every commit that touches code or
tests can shift the `coverage_pct` numbers HEAL classifies. The
`[features.test.coverage].post_commit_refresh` config field lets
HEAL's post-commit hook (already installed by `heal init`) re-run
the reporter in the background after every commit, so the next
`heal status` reads fresh `lcov.info` without the user having to
remember.

Show the proposed config edit first (the exact `post_commit_refresh
= "<command>"` line, matching the reporter command from Step c).
Then `AskUserQuestion`:

> Re-run the reporter from the post-commit hook to keep `lcov.info`
> fresh? Default: **Skip**.
>
> - **Apply**: write `post_commit_refresh = "<command>"`. The
>   spawned process is detached — your commit flow doesn't wait —
>   and its output is discarded. Heavy reporters (Rust workspace
>   coverage takes minutes) will keep your machine warm in the
>   background after each commit.
> - **Skip** (Recommended): leave the field unset; you re-run the
>   reporter manually when you want fresh coverage.

Default-recommend Skip when the reporter run in Step c took longer
than ~30 s wall clock (Rust workspace coverage, large monorepos).
Default-recommend Apply when the run was fast (jest / pytest /
small Rust crate) — the trade-off then favours always-fresh
findings over background CPU.

On Apply: `Edit` `.heal/config.toml`. Use the same command shape
as Step c, prefixed with anything the user normally needs in their
shell (e.g. `cargo llvm-cov ... --quiet` to avoid stray progress
bars filling the terminal scrollback). For polyglot repos, chain
the per-language reporters with `&&` (or `;` if independent
failures should not stop the chain).

Re-running the skill after Apply detects the existing
`post_commit_refresh` and skips this step.

## Phase 3 — Propose CI integration

CI changes are a deploy decision. **Propose only.** Print the
GitHub Actions / GitLab CI / CircleCI block matching the chosen
reporter, with the repo's existing pin style (SHA-pinned actions,
`--locked` flags, version pins) when detectable.

`AskUserQuestion` once:

> Apply this CI block to `<workflow file>`?
>
> - **Apply**: edit the workflow file.
> - **Skip** (Recommended): print only; the user pastes it
>   themselves when they're ready to deploy.

Default-recommend Skip — CI workflow edits affect every
contributor's PR; the install + edit + run loop above only
affects the local machine and is reversible.

### GitHub Actions skeleton

```yaml
coverage:
  name: coverage
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@<sha> # vN
    - uses: dtolnay/rust-toolchain@<sha> # stable
      with:
        components: llvm-tools-preview        # Rust only
    - uses: Swatinem/rust-cache@<sha> # v2     # Rust only
    - run: cargo install cargo-llvm-cov --locked --version <pinned>
    - run: cargo llvm-cov --workspace --lcov --output-path lcov.info --locked --ignore-run-fail
    - uses: actions/upload-artifact@<sha> # vN
      with:
        name: lcov.info
        path: lcov.info
        retention-days: 14
```

Replace the `Rust only` lines and the `cargo install` /
`cargo llvm-cov` block with the per-language equivalents
(`pytest --cov ...`, `jest --coverage ...`,
`go test -coverprofile=... && gcov2lcov ...`,
`sbt clean coverage test coverageReport`).

### GitLab CI

```yaml
coverage:
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

The artifact is so a downstream HEAL run (or a maintainer pulling
the lcov locally) can `heal status --refresh` against the same
`lcov.info` and see findings identical to anyone else on the same
commit.

## Output format

End with one summary block:

```
Detected:        Rust workspace + Python pipeline (polyglot)
Reporter:        cargo-llvm-cov 0.8.5 (Rust)  pytest-cov 5.0 (Python)
Config edits:    [features.test.coverage].enabled  false → true
                 lcov_paths: defaults sufficient (no edit)
                 post_commit_refresh: skipped (Rust run >30 s)
Reporter runs:   lcov.info       18 410 lines  (Rust, --ignore-run-fail
                                                  silenced 1 flaky test)
                 coverage/lcov.info  4 230 lines  (Python)
heal status:     coverage_pct  critical=26  high=1  ok=49  (test family)
CI proposal:     printed above; not applied (run again with the
                 workflow file open if you want to apply).

Follow-ups for the user:
  - Pin cargo-llvm-cov in mise.toml (suggested edit shown above).
  - Investigate the flaky test under instrumentation if it persists.

Next:
  /heal-test-review   # architectural reading + TODO list
  /heal-test-patch    # mechanical drain, one commit per finding
```

## Constraints

- **Per-step approval, no exception.** Every install, every config
  edit, every reporter run, and every CI edit goes through
  `AskUserQuestion`. Default to skip on no answer.
- **CI edits stay proposals by default.** Recommend `Skip` on the
  CI question; only edit `.github/workflows/*.yml`,
  `.gitlab-ci.yml`, or `.circleci/config.yml` when the user
  explicitly chooses Apply.
- **No coverage thresholds.** That's `[calibration.coverage_pct]`
  territory, not this skill.
- **Match default `lcov_paths` when possible.** Only extend the
  list when the reporter's default sits outside the four
  defaults (Scala / scoverage, custom output dirs).
- **Polyglot supported.** Multiple language markers → run Phase 2
  per ecosystem. Each ecosystem can be opted out of independently.
- **Idempotent re-run.** Re-invoking after a partial run picks up
  where it stopped. Detect installed reporters, fresh
  `lcov.info`, already-flipped `enabled`, an existing
  `post_commit_refresh`, and skip those steps.
- **`--ignore-run-fail` for Rust.** cargo-llvm-cov instrumentation
  can break tests that pass under plain `cargo test` (env-var
  leakage to spawned subprocesses is the most common cause).
  Always use `--ignore-run-fail` to still produce `lcov.info`;
  print failing test names so the user can investigate.
- **English output.** Skill writes English; CI / config files
  may be in any language.
