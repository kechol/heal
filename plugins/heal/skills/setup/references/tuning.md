# Tune the core config

Loaded by `/heal:setup` for the config step: the first run after
`heal init`, a codebase whose shape changed (a rewritten layer, a new
generated tree, a new primary language), or a user who wants heal
stricter or more lenient. Writes only `.heal/config.toml`; load
`config.md` (next to this file) before recommending a non-default
value. Five phases: calibrate, survey, workspaces (monorepos only),
strict-fit check, choose strictness, write.

### Phase 1 — Calibrate

Read `calibration` from `heal doctor --json`. When `reasons` is empty,
keep the calibration and read its breaks with `heal calibrate --json`.
When a rule fired, name it (`commits_since_calibration`,
`file_count_drift`, `graduated`) with the numbers doctor reported and
ask whether to rebuild. Only on a yes, run:

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
