---
name: heal-config
description: Calibrate the codebase, survey its shape, and write or update `.heal/config.toml` with thresholds tuned to a strictness level the user picks (Strict / Default / Lenient). Read-only on the codebase; writes only `.heal/config.toml`. Trigger on "set up heal config", "tune heal thresholds", "make heal stricter / more lenient", "/heal-config".
---

# heal-config

One-shot skill that produces (or updates) a project's
`.heal/config.toml`. It works in three phases: **calibrate** so the
percentile breaks match this codebase, **survey** so excludes / metric
toggles match this codebase's shape, and **write** the config tuned to
a strictness level the user chose.

The skill is **language-agnostic** — it consults `heal metrics --json`
to see which observers fired and what the per-language LOC mix is, then
shapes the config accordingly. It does not assume Rust / TS / Python.

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

## Pre-flight

Before changing anything:

1. **Project initialised.** Run `heal init --no-skills --json` if
   `.heal/` doesn't exist yet. Capture the resulting paths.
2. **Calibration fresh.** Run
   `heal calibrate --force --json` so the percentile breaks reflect
   the *current* codebase. The skill needs the up-to-date breaks to
   reason about whether a metric has signal at all
   (see `references/config.md` § "Calibration interplay").
3. **Capture the survey.** Run
   `heal metrics --json` and `heal check --refresh --json`. Both feed
   the survey phase.
4. **Worktree state noted.** A dirty worktree is fine for *reading*
   the codebase, but the calibration scan should reflect committed
   state. Tell the user once if `worktree_clean: false` shows up in
   the check JSON; don't refuse.

## Procedure (Calibrate → Survey → Choose → Write)

### Phase 1 — Calibrate

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
3. **`heal check --refresh --json`.** Specifically `findings[]`:
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
   excludes the user added, custom `policy.rules`, language
   preference). Do not silently overwrite.
2. **Apply the recipe** — set the knobs from the strictness table.
3. **Apply the survey** — fill `git.exclude_paths` with the directories
   from Phase 2; set per-metric `enabled = false` for the disable
   candidates *only when the calibration confirmed no signal*; set
   per-metric tunes from the "Tune candidates" list.
4. **Validate.** `Config::from_toml_str` (the heal binary's loader)
   uses `deny_unknown_fields`, so a typo will surface immediately.
   The simplest sanity check is to call `heal check --refresh --json`
   after writing — if the file is malformed `heal` will fail with a
   precise schema error before the scan starts.
5. **Show the diff.** Don't just write. Render a short summary of:
   - What changed vs the previous config.
   - What `heal check --refresh --json` reports as the new
     `severity_counts`.
   - Whether any previously-flagged findings now classify as Ok (a
     loosening) or Critical (a tightening).

## Output format

End with three short blocks:

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
  - metrics.lcom.enabled: true → false           # no classes detected

Effect:
  before: critical=3 high=11 medium=22 ok=0
  after:  critical=4 high=15 medium=18 ok=0
  → 1 finding promoted to critical, 4 medium reclassified as high.
  Run `heal check --refresh` to inspect the new ranking.
```

## Constraints

- **Write `.heal/config.toml` only.** Never edit `calibration.toml`
  directly — recalibrating is `heal calibrate --force`.
- **Do not overwrite user customisations the recipe doesn't touch.**
  Merge, don't replace.
- **Recommend, don't require.** If the user later edits the file by
  hand, the next run of this skill should re-apply the recipe but keep
  hand-edits to keys outside the recipe table.
- **`deny_unknown_fields` is on.** Typos break the loader. After
  writing, run `heal check --refresh --json` once to confirm the file
  parses; if it fails, surface the error and revert.
- **English output by default.** The user can ask for translation if
  they prefer another language. The `[project].response_language`
  setting controls heal's *own* output language, not the skill's.
