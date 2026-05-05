---
name: heal-setup
description: One-shot setup wizard. Calibrates the codebase, surveys its shape, writes `.heal/config.toml` at a strictness level the user picks (Strict / Default / Lenient), and asks whether to enable the optional `[features.docs]` and `[features.test]` families — chaining to `/heal-doc-pair-setup` and `/heal-test-reporter-setup` when the user opts in. Read-only on the codebase; writes only `.heal/config.toml`. Trigger on "set up heal", "configure heal", "tune heal thresholds", "make heal stricter / more lenient", "enable heal docs", "enable heal coverage", "/heal-setup".
---

# heal-setup

One-shot skill that produces (or updates) a project's
`.heal/config.toml` and offers to wire up the optional feature
families. It works in five phases: **calibrate** so the percentile
breaks match this codebase, **survey** so excludes / metric toggles
match this codebase's shape, **choose** a strictness level, **write**
the tuned config, then **gate** the optional `[features.docs]` and
`[features.test]` families with `AskUserQuestion` (chaining to the
companion setup skills when the user opts in).

The skill is **language-agnostic** — it consults `heal metrics --json`
to see which observers fired and what the per-language LOC mix is, then
shapes the config accordingly. It does not assume any specific
language stack (Rust / TypeScript / JavaScript / Python / Go /
Scala are all first-class).

Read-only on source files. The only file it writes is
`.heal/config.toml`. Calibration thresholds live in
`.heal/calibration.toml` and are produced by `heal calibrate --force`,
not by this skill.

## When this skill is right

- First-time setup right after `heal init`: the default config is
  generic; this skill tunes it.
- The codebase's shape changed (rewrote a layer, vendored a new
  generated tree, switched primary language) and the previous config
  no longer fits.
- The user wants to make heal stricter (new project, quality bar) or
  more lenient (legacy import, gradual rollout).

If the user just wants to know what a single setting does, point them
at `references/config.md` directly — this skill is for *deriving* a
config, not explaining one.

## References (load on demand)

- `references/config.md` — complete reference for every key in
  `.heal/config.toml`: type, default, what it controls, when to tune.
  Load it before recommending a non-default value.

## Output language

Write the survey summary, the strictness recommendation, the
`AskUserQuestion` prompts, and the post-write report in the user's
language. Resolution order:

1. Explicit instruction in the current conversation.
2. The language the user is writing in (Claude Code's conversation
   language).
3. `[project].response_language` in `.heal/config.toml` (free-form:
   `"Japanese"`, `"日本語"`, `"ja"`, `"français"`). When this skill
   *writes* `response_language` for the first time, infer it from
   signals (1) and (2) and confirm with the user before persisting —
   the value flows to every other heal skill on subsequent runs.
4. English (fallback).

Identifiers stay verbatim — config keys (`[project]`,
`[features.docs]`, `[metrics.ccn]`), command names (`heal init`,
`heal calibrate --force`), file paths (`.heal/config.toml`,
`.heal/calibration.toml`), and TOML values written into the file
are part of the contract. Translate the surrounding explanation,
not the file.

## Pre-flight

Before changing anything:

1. **Project initialized.** Run `heal init --no-skills --json` if
   `.heal/` doesn't exist yet. Capture the resulting paths *and* the
   `monorepo_signals` field — it tells Phase 2.5 whether to run.
2. **Calibration fresh.** Run
   `heal calibrate --force --json` so the percentile breaks reflect
   the *current* codebase. The skill needs the up-to-date breaks to
   reason about whether a metric has signal at all
   (see `references/config.md` § "Calibration interplay").
3. **Capture the survey.** Run
   `heal metrics --json` and `heal status --refresh --json`. Both feed
   the survey phase.
4. **Worktree state noted.** A dirty worktree is fine for *reading*
   the codebase, but the calibration scan should reflect committed
   state. Tell the user once if `worktree_clean: false` shows up in
   the status JSON; don't refuse.

## Procedure (Calibrate → Survey → Choose → Write → Feature gates)

### Phase 1 — Calibrate

Run the recalibration drift check first (see *Recalibration drift
check* below). When zero drift conditions fire, skip `--force` and
read the existing calibration via `heal calibrate --json` — the
percentile breaks are still valid. When any condition fires, run:

```sh
heal calibrate --force --json
```

Capture the JSON. Note for the survey:

- Which metrics returned a `MetricCalibration` (i.e. fired). A metric
  whose calibration is missing or has `NaN` percentiles has too few
  samples — recommend leaving it `enabled = true` but expect the
  scan-time floor (e.g. `change_coupling.min_coupling`) to do the
  filtering.
- The `floor_critical` / `floor_ok` defaults that already apply (see
  `core::calibration` in `references/config.md`).
- `meta.codebase_files` — the survey uses this to decide whether
  thresholds like `min_coupling = 3` are appropriate (small repo) or
  conservative (large repo).

### Phase 2 — Survey

Read these without editing:

1. **Top-level layout.** A single `ls` of the repo root + one level
   down is enough. You're looking for: build/dist/dependency trees,
   generated code (`*.pb.go`, `__pycache__`, `node_modules`), vendored
   dependencies, fixtures, snapshot directories.
2. **`heal metrics --json`.** Look at:
   - `loc.languages` — primary language; long tail.
   - `severity_counts` — does the project have a complexity problem,
     a duplication problem, a coupling problem? The dominant axis
     informs which metric's strictness to dial up.
   - Per-metric top lists — which file paths repeatedly show up as
     hotspots / churn / duplication.
3. **`heal status --refresh --json`.** Specifically `findings[]`:
   - Files that show up in many findings → probably structural
     hotspots; don't exclude them, they are the point.
   - Files that show up in *one* metric's findings only because of
     parser tables / generated code / fixtures → exclude-path
     candidates.
   - `change_coupling` pairs spanning module roots → keep enabled.
     Pairs that are all `tests/* ↔ src/*` → tune
     `change_coupling.min_lift` up so test ↔ implementation pairs
     drop out.

Build a short list of:

- **Exclude paths.** The set of directories whose findings are
  intrinsic (parser tables, generated code, fixtures, vendored deps).
  These go into `git.exclude_paths` (so all observers honor them via
  `metrics.loc.inherit_git_excludes = true`).
- **Disable candidates.** Metrics with no signal on this codebase
  (e.g. `lcom` on a repo with no classes; `change_coupling` on a
  solo-author repo with no shared edits). Tighter than excluding —
  prefer leaving enabled unless the calibration showed `NaN`s.
- **Tune candidates.** Metrics where the default `min_*` thresholds
  are too loose / too tight given the survey.

### Phase 2.5 — Workspaces (monorepos only)

Run only when `heal init --json` reported `monorepo_signals` (or when
the user explicitly mentioned a monorepo / package layout). Skip
silently for solo packages — declaring workspaces on a flat repo just
adds noise.

The goal: turn the detected manifest entries into a concrete
`[[project.workspaces]]` block. Each workspace gets its own
calibration cohort, so percentile breaks for `pkg/web` no longer get
dragged around by `pkg/api`'s outliers.

Steps:

1. **Enumerate workspace directories.** Read whichever manifest the
   detector flagged:
   - `package.json` → the `workspaces` array (or `workspaces.packages`).
     Glob `pkg/*` patterns to actual existing directories.
   - `pnpm-workspace.yaml` → the `packages:` list (yaml — read as
     text, parse the `- 'pattern'` lines).
   - `Cargo.toml` → `[workspace] members`. Globs work the same way
     as npm.
   - `go.work` → the `use (...)` block, one path per line.
   Drop entries whose directories don't exist on disk (manifests can
   list aspirational paths). Keep the result deterministic — sort
   alphabetic.

2. **Confirm with the user.** One `AskUserQuestion`:

   ```
   Question: "Declare these workspaces in heal config? Per-workspace
              calibration scopes percentile breaks per package."
   Options:
     - "Declare all": write every detected directory.
     - "Pick subset": ask which ones to keep (skip top-level scripts/,
       tools/ if they're included in a glob).
     - "Skip": leave `[[project.workspaces]]` empty and continue with
       repo-wide calibration.
   ```

   Don't auto-declare without confirmation — the cohort split changes
   every existing finding's severity (calibration shifts under the
   user). Make the consequence visible.

3. **Pick `language` per workspace.** Run
   `heal metrics --json --workspace <path>` per declared workspace
   and read the LOC primary. If it differs from the repo-wide primary,
   set `language` on the workspace overlay so the change-coupling
   pair-class noise filter (per-language lockfiles and build-output
   dirs — `Cargo.lock` / `target/` for Rust, sbt's `target/` for
   Scala, `.egg-info/` for Python, etc.) matches reality.

4. **Tune per-workspace recipes** (skip when no override is needed):
   - **`exclude_paths`**: workspace-relative `.gitignore` lines layered
     on top of `git.exclude_paths`. Same DSL as global excludes — glob
     (`*`, `**`, `?`, `[abc]`), directory-only (`foo/`), root-anchor
     (`/foo` from the workspace root), negation (`!keep`), `#`
     comments. `exclude_paths = ["third_party/"]` under
     `path = "pkg/api"` excludes `pkg/api/**/third_party/` only; other
     workspaces are unaffected.
   - **Calibration floor overrides**: per-metric `floor_critical` /
     `floor_ok` per workspace. Use when one workspace runs cleaner or
     legacier than the rest:
     ```toml
     [[project.workspaces]]
     path = "pkg/legacy"

     [project.workspaces.metrics.ccn]
     floor_critical = 40   # gentler than the global 25
     floor_ok = 18         # raise graduation gate too
     ```
     Applied *after* the global `[metrics.<m>]` floors, so workspace
     values win when both are set. Supported metrics: `ccn`,
     `cognitive`, `duplication`, `change_coupling`, `lcom`.
   - **Scan-time tunables** like `[metrics.churn] since_days` are
     *not* yet per-workspace; the global value applies to every
     workspace.

5. **Write the block.** Append `[[project.workspaces]]` entries to
   `.heal/config.toml` (preserve other user-set keys; merge, don't
   replace). Validate by re-running `heal status --refresh --json`
   and confirming `workspaces[]` in the result reflects the new shape.

Cross-workspace coupling lands in its own Advisory bucket (metric
`change_coupling.cross_workspace`) and never enters the drain queue
without an explicit
`[policy.drain.metrics."change_coupling.cross_workspace"]` override.
Mention this once in the post-write summary so the user understands
what to watch.

### Phase 2.7 — Strict-fit check

Before offering Strict in Phase 3, compare this codebase's
calibration against the Strict recipe. Strict only adds value when
the codebase actually breaches its floors; on a codebase that's
already simpler than the Strict gate, Strict floods T0 with
proxy-metric noise instead of surfacing real targets.

The mechanism: a value `>= floor_ok` exits the floor cascade and
enters the percentile classifier; if it's also `>= p95`, it lands at
Critical immediately. So when `Strict.floor_ok > calibration.p95`,
**every value barely above the gate jumps straight to Critical via
the percentile cascade** — leaving Medium / High effectively empty
and Critical flooded with normal codebase code.

For each metric in `{ccn, cognitive}`, compare:

- `Strict.<metric>.floor_ok` (from the recipe table)
- `calibration.<metric>.p95` (from `heal calibrate --json`'s
  `calibration.calibration.<metric>.p95`)

If `Strict.floor_ok > calibration.p95` for any metric, Strict floods
that axis. Note it for Phase 3.

For `duplication`, the same logic uses `floor_critical` (no
`floor_ok` exists for that metric): if
`Strict.duplication.floor_critical > calibration.duplication.p95`,
Strict's Critical line sits above the codebase's natural top —
not a flood (Strict adds nothing here), just a no-op axis.

Two illustrative cases:

| codebase                          | ccn p95 | Strict floor_ok | verdict                                    |
|-----------------------------------|---------|-----------------|--------------------------------------------|
| simple CLI (heal itself)          | 7       | 8               | floods — recommend Default                 |
| typical web app                   | 12      | 8               | safe — Strict's gate sits below p95        |
| greenfield with strict review     | 5       | 8               | floods on every metric — recommend Default |

Build a short list of metrics that flood. The list goes into the
strictness question below — do **not** silently demote Strict.

### Phase 3 — Choose strictness

Use `AskUserQuestion` to pick one of three levels. Frame it once,
plainly:

```
Question: "How strictly should heal flag findings on this codebase?"
Options:
  - "Strict": new projects or anything where you want an aggressive
    quality bar — proxy-metric floors lowered, drain queue includes
    `high:hotspot`, more metrics enabled by default.
  - "Default" (recommended): the shipped defaults — McCabe / Sonar
    literature floors, drain queue is `critical:hotspot` only.
  - "Lenient": legacy imports or gradual rollouts — proxy-metric
    floors raised, drain queue restricted to Critical-only, Medium
    surfaced quietly.
```

When Phase 2.7 flagged Strict as flooding on this codebase, prepend
a warning to the `description` field — name the metrics and the
numbers. Example:

> ⚠ Strict's `ccn.floor_ok=8` sits above this codebase's `ccn p95=7`.
> Every CCN ≥8 would land at Critical via the percentile cascade
> (the Medium / High band is empty). Default's literature floors fit
> this codebase's actual shape; pick Strict only if the goal is to
> flag *every* function above CCN=8 as Critical.

Keep the warning short and factual. Don't refuse Strict — surface the
trade-off and let the user choose.

Don't combine with other questions. If the user picks `Other`, ask one
follow-up to pin down what they want to relax or tighten — do not
silently default.

The strictness level maps to specific knobs (full table in
`references/config.md` § "Strictness recipes"):

| Knob                                 | Strict     | Default     | Lenient    |
|--------------------------------------|------------|-------------|------------|
| `metrics.ccn.floor_ok`               | 8          | (literature: 11) | 14    |
| `metrics.ccn.floor_critical`         | 20         | (literature: 25) | 30    |
| `metrics.cognitive.floor_ok`         | 5          | (literature: 8)  | 12    |
| `metrics.cognitive.floor_critical`   | 35         | (literature: 50) | 60    |
| `metrics.duplication.floor_critical` | 20         | (literature: 30) | 40    |
| `metrics.duplication.min_tokens`     | 35         | 50               | 75    |
| `metrics.change_coupling.min_lift`   | 1.5        | 2.0              | 3.0   |
| `[policy.drain].must`                | `["critical:hotspot", "high:hotspot"]` | `["critical:hotspot"]` | `["critical:hotspot"]` |
| `[policy.drain].should`              | `["critical", "high"]` | `["critical", "high:hotspot"]` | `["critical"]` |

Anything `references/config.md` doesn't list per-strictness stays at
its shipped default.

### Phase 4 — Write

Build the config in memory, then write it:

1. **Read the current `config.toml`** if one exists. Preserve any
   user-set keys the strictness recipe doesn't touch (free-form
   excludes the user added, language preference). Do not silently
   overwrite.
2. **Apply the recipe** — set the knobs from the strictness table.
3. **Apply the survey** — fill `git.exclude_paths` with the directories
   from Phase 2; append the disable candidates to
   `[metrics] disabled = [...]` *only when the calibration confirmed
   no signal* (`loc` cannot be disabled); set per-metric tunes from
   the "Tune candidates" list.
4. **Validate.** `Config::from_toml_str` (the heal binary's loader)
   uses `deny_unknown_fields`, so a typo will surface immediately.
   The simplest sanity check is to call `heal status --refresh --json`
   after writing — if the file is malformed `heal` will fail with a
   precise schema error before the scan starts.
5. **Show the diff.** Don't just write. Render a short summary of:
   - What changed vs the previous config.
   - What `heal status --refresh --json` reports as the new
     `severity_counts`.
   - Whether any previously-flagged findings now classify as Ok (a
     loosening) or Critical (a tightening).

### Phase 5 — Feature gates (`[features.docs]`, `[features.test]`)

Both feature families ship disabled by default. After the core config
is written, ask the user whether to enable each one — separately, in
two `AskUserQuestion` rounds — and then chain to the companion setup
skill when they opt in.

Skip the prompt for a feature whose `[features.<name>] enabled = true`
already (the user enabled it earlier; re-prompting is busy-work).
Surface that fact in the post-write summary instead.

#### Step 1 — `[features.docs]` gate

The docs family runs the doc ⇔ src drift / freshness / link / orphan
/ todo / coverage observers. It needs `.heal/doc_pairs.json` (Layer A)
and the `[features.docs.standalone]` glob set (Layer B) to do anything
useful. Generation of `doc_pairs.json` is the responsibility of
`/heal-doc-pair-setup`; this skill writes the toggle and the
standalone globs.

1. **Survey first** so the question is informed:

   - List directories that look like prose docs:
     `docs/`, `documentation/`, `doc/`, `book/`, `wiki/`, `guide/`,
     `handbook/`. Glob for `**/*.md` and `**/*.rst` at the repo
     root and report the count.
   - Note governance files that should be excluded
     (`CHANGELOG*`, `CONTRIBUTING*`, `CODE_OF_CONDUCT*`,
     `SECURITY*`, `**/adr/**`) and build artefacts (`target/`,
     `dist/`, `node_modules/`).
   - If the project is **doc-light** (≤ 5 Markdown / RST files,
     or no top-level `docs/` tree, or pure README + a couple of
     ADRs), say so in the question — `[features.docs]` won't
     surface much signal. Don't refuse, just inform.

2. **Ask once** with `AskUserQuestion`:

   ```
   Question: "Enable [features.docs]? It surfaces doc-side findings
              (drift vs source, stale freshness, broken links, orphans,
              TODO density, coverage of public symbols)."
   Options:
     - "Enable": flip [features.docs].enabled = true, populate
                 [features.docs.standalone] with this codebase's doc
                 layout, then chain to /heal-doc-pair-setup.
     - "Skip":   leave [features.docs] disabled. Re-runnable by
                 invoking /heal-setup again later.
   ```

   Prefix the question with the survey result one-liner — e.g. *"42
   Markdown files under `docs/`, 7 RST files under `book/`."*

3. **On Enable** — write the standalone block from the survey, not
   the shipped defaults. The shipped defaults (`include = ["**/*.md",
   "**/*.rst"]`) work, but a tighter `include` keeps `heal status`
   faster on monorepos where Markdown is also embedded in
   `node_modules/` and lockfile generators. Build the entries:

   - `include`:
     - Always: `**/*.md`, `**/*.rst`.
     - When the project has a single dedicated docs tree
       (`docs/` only), prefer the narrower `docs/**/*.md` / `**/*.rst`
       to keep the universe tight. Otherwise leave the broad globs.
   - `exclude`: layer the project's own ignored dirs on top of the
     shipped baseline:
     - Always include the shipped defaults
       (`CHANGELOG*`, `CONTRIBUTING*`, `CODE_OF_CONDUCT*`,
       `SECURITY*`, `**/adr/**`, `target/**`, `dist/**`,
       `node_modules/**`).
     - Add any project-specific generated doc trees the survey
       found (e.g. `docs/api/generated/**` for cargo-doc / sphinx
       html, `site/**` for Jekyll / Hugo build output).
     - Add language-specific build dirs the survey found that the
       shipped defaults miss (e.g. `_build/**` for sphinx,
       `public/**` for Hugo, `.next/**` for Next.js, `vendor/**`
       for Rails).

   Write the resulting block as `[features.docs]` + a nested
   `[features.docs.standalone]` if (and only if) `include` /
   `exclude` differ from defaults. Skip the section entirely when
   the user's set matches the shipped defaults — the strict
   serializer drops it.

4. **Chain to `/heal-doc-pair-setup`.** After the config write
   succeeds, instruct the agent to invoke the companion skill in
   the same session so `.heal/doc_pairs.json` lands before the
   next `heal status` run. Don't run the chained skill silently —
   announce the hand-off so the user can stop the chain if they
   want to edit the standalone globs first.

#### Step 2 — `[features.test]` gate

The test family runs the `coverage_pct` and `skip_ratio` observers
plus tags every Finding's `is_test_file` flag against
`test_paths`. The lcov reporter is a separate, language-specific
setup that lives in `/heal-test-reporter-setup`; this skill writes
the toggle, `test_paths`, and `lcov_paths`.

1. **Survey first**:

   - Detect language stacks present in the repo root:
     `Cargo.toml`, `pyproject.toml` / `setup.py` / `requirements.txt`,
     `package.json`, `go.mod`, `build.sbt`. Note each ecosystem
     present.
   - Find existing test directories / files: `tests/`, `test/`,
     `__tests__/`, `**/*_test.go`, `**/*.test.ts`,
     `**/*.spec.ts`, `**/test_*.py`, `**/*_test.py`,
     `**/*Test.scala`, `**/*Spec.scala`. Report what exists.
   - Probe for an existing lcov file at the four shipped defaults
     (`lcov.info`, `coverage/lcov.info`,
     `target/llvm-cov/lcov.info`,
     `coverage/lcov-report/lcov.info`). Note which (if any) exist
     and whether they're recent.
   - If the project has **no test files at all**, say so in the
     question — coverage will be 0% across the board and skip-ratio
     has nothing to count. Don't refuse, just inform.

2. **Ask once** with `AskUserQuestion`:

   ```
   Question: "Enable [features.test]? It tags every Finding with
              is_test_file, and (when [features.test.coverage] is on)
              ingests lcov.info to surface low-coverage hotspots and
              skip-ratio drift."
   Options:
     - "Enable": flip [features.test].enabled = true, populate
                 test_paths and lcov_paths from the survey, then chain
                 to /heal-test-reporter-setup.
     - "Skip":   leave [features.test] disabled. Re-runnable by
                 invoking /heal-setup again later.
   ```

   Prefix with the survey one-liner — e.g. *"Rust + TypeScript
   stack; tests under `tests/` and `**/*.test.ts`; no `lcov.info`
   present yet."*

3. **On Enable** — write the toggle block from the survey, not the
   shipped defaults. The shipped defaults cover the broad
   per-ecosystem conventions; a tighter list keeps `is_test_file`
   tagging precise on a single-language repo.

   - `test_paths`: keep only the patterns whose ecosystem markers
     showed up in the survey. A pure-Rust repo doesn't need the JS
     `**/*.test.ts` family or the Scala `*Test.scala` family —
     those globs are dead weight on every classification call.
     For polyglot repos, keep every matching ecosystem's set.
   - `lcov_paths`: keep the shipped defaults unless the survey
     found an existing lcov in a non-default location (e.g.
     `target/scala-2.13/scoverage-report/lcov.info` for sbt with
     scoverage). When extending, append — don't replace.

   Write the resulting block as `[features.test]` + a nested
   `[features.test.coverage]` if (and only if)
   `lcov_paths` / `coverage.enabled` differ from defaults.

4. **Chain to `/heal-test-reporter-setup`.** After the config
   write, hand off so the language-specific reporter wiring
   (cargo-llvm-cov, pytest-cov, vitest / jest, gcov2lcov,
   scoverage) lands. The chained skill installs the reporter,
   flips `[features.test.coverage].enabled = true`, runs the
   reporter, and verifies — each step gated by its own
   `AskUserQuestion`. Same hand-off etiquette as the docs family:
   announce the chain, let the user opt out of running it
   immediately. CI workflow edits stay a proposal there too;
   this skill never edits CI.

## Output format

End with four short blocks:

```
Calibration:
  codebase_files: 142
  ccn p95:        21.7
  cognitive p95:  53.0
  hotspot p90:    67.0
  (anything missing → "no signal — relying on scan-time floor")

Config changes:
  - git.exclude_paths += ["vendor/", "src/generated/"]
  - metrics.duplication.min_tokens: 50 → 35      # strict mode
  - metrics.change_coupling.min_lift: 2.0 → 1.5  # strict mode
  - policy.drain.must = ["critical:hotspot", "high:hotspot"]   # strict mode
  - metrics.disabled += ["lcom"]                 # no classes detected

Feature gates:
  - [features.docs]: enabled  → chained /heal-doc-pair-setup
  - [features.test]: skipped (user declined; re-run /heal-setup to revisit)

Effect:
  before: critical=3 high=11 medium=22 ok=0
  after:  critical=4 high=15 medium=18 ok=0
  → 1 finding promoted to critical, 4 medium reclassified as high.
  Run `heal status --refresh` to inspect the new ranking.
```

## Recalibration drift check (idempotent)

HEAL does not auto-trigger recalibration anymore — there's no event
log to watch. Whenever the user invokes this skill, decide whether to
suggest a recalibration *before* doing anything else, using only:

1. `heal calibrate --json` (no `--force`) — surfaces
   `meta.calibrated_at_sha`, `meta.calibrated_at_files`, and
   `meta.created_at` from the existing `calibration.toml`.
2. `heal status --refresh --json` — surfaces the current Critical /
   High counts and the live finding list.
3. `git rev-list <calibrated_at_sha>..HEAD --count` — commits since
   the calibration was built. Skip when `calibrated_at_sha` is missing
   (legacy file).
4. `.heal/findings/fixed.json` — how many findings the user has marked
   as resolved since last calibration.

Suggest `heal calibrate --force` when **any** of these fire:

- `commits since calibration > 200` (the codebase has moved enough that
  the percentile breaks may no longer reflect today's distribution).
- `|current_codebase_files - calibrated_at_files| / calibrated_at_files
  > 0.20` (file count drifted ≥20%).
- `critical == 0 && high == 0` for the current `latest.json` AND the
  fixed map shows ≥10 entries since calibration (codebase has
  graduated; thresholds may now be too lenient).

When none fire, say so and move on — don't recalibrate proactively.

This check is idempotent and read-only. The user always has the final
say on whether to run `heal calibrate --force`.

## Constraints

- **Write `.heal/config.toml` only.** Never edit `calibration.toml`
  directly — recalibrating is `heal calibrate --force`. Never write
  `.heal/doc_pairs.json` directly — that's `/heal-doc-pair-setup`'s
  job. Never write CI files or reporter configs — that's
  `/heal-test-reporter-setup`.
- **Do not overwrite user customisations the recipe doesn't touch.**
  Merge, don't replace.
- **Recommend, don't require.** If the user later edits the file by
  hand, the next run of this skill should re-apply the recipe but keep
  hand-edits to keys outside the recipe table.
- **`deny_unknown_fields` is on.** Typos break the loader. After
  writing, run `heal status --refresh --json` once to confirm the file
  parses; if it fails, surface the error and revert.
- **Feature gates default to skip on no answer.** If the user
  declines or doesn't reply to a `[features.docs]` /
  `[features.test]` question, leave the section omitted (= disabled
  by default). Never auto-enable a feature whose chained setup the
  user hasn't acknowledged.
- **Conversation language follows the resolution order in
  "Output language" above.** The skill's prose, prompts, and report
  match the user's language. When `[project].response_language` is
  absent and the user is clearly writing in a non-English language,
  set it during the write phase (after confirming with the user) so
  every other heal skill picks the same default on subsequent runs.
