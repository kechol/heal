# CLI commands

Per-subcommand internal contract. For canonical command names see
`glossary.md`. For data shapes see `data-model.md`.

CLI entrypoint: `main.rs:5` → `Cli::parse().run()` → `commands::*`.
All handlers return `anyhow::Result<()>`. `?` bridges
`core::Error → anyhow::Error`.

Global flag: `--project <PATH>` (default: current working directory).

---

## `heal init`

```
heal init [--force] [--yes] [--no-skills] [--json]
```

Lifecycle (`commands/init.rs`):

1. `paths.ensure()` — create `.heal/` and `.heal/findings/` dirs.
2. Write `.heal/.gitignore` (idempotent — only writes if body differs).
3. Write `.heal/config.toml` (skipped unless `--force` or absent;
   returns `ConfigAction::{Wrote, Overwrote, KeptExisting}`).
4. Install `.git/hooks/post-commit`. Detects existing HEAL marker
   (`HEAL_HOOK_MARKER = "# heal post-commit hook"`); refreshes only if
   marked. Skips user-authored hooks unless `--force`. `chmod 0o755` on
   Unix.
5. Run initial observer scan (`run_all`); build calibration with
   `calibrated_at_sha` + `codebase_files` metadata; write
   `calibration.toml`.
6. Optionally install bundled skills. Decision tree:
   - `--no-skills` → skip.
   - `claude` not on `PATH` → skip.
   - `--yes` → install.
   - stdin is TTY → prompt (default `Y`).
   - else → skip with non-interactive hint.
7. Sweep legacy `heal hook edit` / `heal hook stop` from
   `.claude/settings.json` if present (via `claude_settings::wire`).

**Hook script written:**

```sh
#!/usr/bin/env sh
# heal post-commit hook
if command -v heal >/dev/null 2>&1; then
  heal hook commit || true
fi
exit 0
```

Failures swallowed (`|| true`) so a broken HEAL never blocks a commit.

**Output (text):** init outcome summary, primary language, severity
counts, monorepo signals.

**Output (JSON):** `InitReport` (action enums, version, severity
counts).

**Exit:** 0 success; anyhow error otherwise.

---

## `heal hook <event>`

```
heal hook commit | edit | stop
```

`commit` is the only live event; `edit` and `stop` are silent no-ops
(back-compat with stale `settings.json`).

`commands/hook.rs:run_commit`:

1. **Silent no-op if `.heal/` does not exist** — prevents a stale hook
   from materialising `.heal/` in an un-opted-in worktree.
2. Load config (silent exit if missing — uncalibrated project).
3. `run_all` observers → `classify_with_calibration` →
   `write_nudge`.

**Nudge format** (single line, post-commit output stays compact):

- No calibration → silent (no output at all).
- 0 critical/high → `heal: recorded · clean`.
- Has critical/high → `heal: recorded · X critical, Y high · heal status`.

ANSI colors when stdout is a TTY.

---

## `heal status`

```
heal status [--metric <FindingMetric>] [--workspace <PATH>]
            [--feature <PREFIX>] [--severity <SeverityFilter>]
            [--all] [--json] [--refresh] [--top <N>] [--no-pager]
```

Pipeline (`commands/status.rs:44-112`):

1. Read cache from `.heal/findings/latest.json` unless `--refresh`.
2. Idempotency check: `is_fresh_against(head_sha, config_hash,
   worktree_clean)`. Match → reuse cached record. Mismatch or dirty
   worktree → recompute.
3. Recompute path: `build_record(...)` → `run_all` → `classify` →
   `FindingsRecord` → `fs::atomic_write` to `latest.json`.
4. `reconcile_fixed(fixed.json, regressed.jsonl, &record)` — re-detected
   fixes move to the audit trail.
5. Render through pager when stdout is TTY, not JSON, not `--no-pager`.

**Pager:** spawns `$PAGER` (default `less`) with `LESS=FRX` (auto-exit
short, ANSI pass-through, no alt-screen). Broken pipe on user quit is
swallowed → exit 0.

**Render layout** (top-to-bottom, post-commit `9efded8`):

1. Title + header (calibrated time, finding count, severity inline
   counts at top).
2. Regressed section (re-detected after `mark-fixed`).
3. Drain tier sections (T0 Must 🎯, T1 Should 🟡, Advisory ℹ️).
4. Ok section (only with `--all`).
5. Footer with goal + `/heal-code-patch` nudge.

**Filters:**

- `--metric <FindingMetric>` — `ccn`, `cognitive`, `complexity`
  (CCN+Cognitive), `duplication`, `coupling` (symmetric pairs only),
  `hotspot`, `lcom`.
- `--feature <PREFIX>` — file path prefix (e.g. `src/payments`).
- `--workspace <PATH>` — single declared workspace.
- `--severity <Critical|High|Medium|Ok>` — floor.
- `--all` — show Medium/Ok and low-Severity hotspots.
- `--top <N>` — cap each bucket.

**Output (JSON):** raw `FindingsRecord` (the on-disk shape).

**Exit:** 0 success (broken pipe included); error otherwise.

---

## `heal metrics`

```
heal metrics [--metric <MetricKind>] [--workspace <PATH>] [--json]
```

`commands/metrics/mod.rs`. Fresh recompute, **no cache reuse**. Designed
for CI / scripting; no idempotency contract.

Per-section trait (`MetricSection`) registered in `all_sections()`:
`Loc`, `Complexity`, `Churn`, `ChangeCoupling`, `Duplication`,
`Hotspot`, `Lcom`. Each section provides:

- `render_text(&report) → String` with `top_n` cutoff.
- `raw_json(&report) → serde_json::Value` (full typed report; omitted
  when `--metric` narrows the output).
- `worst_json(&report, top_n) → serde_json::Value` for `--metric X
  --json` (precomputed worst-N payload).

**Output:**

- `--metric X --json` → narrowed payload (worst-N for that metric).
- `--json` (no `--metric`) → full map: `initialized` flag + per-section
  raw payloads. Can be large.

If config is missing → emits `{"initialized": false}` (text: "not
initialized") and exits 0. Doesn't error.

---

## `heal diff [<revspec>]`

```
heal diff [<revspec>=HEAD] [--workspace <PATH>] [--all] [--json]
```

Two paths (`commands/diff.rs`):

1. **Cache hit** — `latest.json.head_sha` matches resolved ref → read
   directly. Fast.
2. **Worktree fallback** — when no cache match:
   - LOC gate: scan current worktree LOC. If
     `> [diff].max_loc_threshold` (default `200_000`), **exit 2** with
     guidance for a manual two-scan workflow.
   - `git worktree add --detach <tmp> <sha>`.
   - Run observers + classify against **current** config + calibration
     ("today's rules applied to historical source" — apples-to-apples).
   - `WorktreeGuard` (RAII `Drop`) tears down the temp worktree even on
     `?` short-circuit.

**Diff buckets** (`struct Diff`):

- `resolved` — in `from`, gone in `to`.
- `regressed` — severity increased.
- `improved` — severity decreased.
- `new_findings` — in `to`, absent in `from`.
- `unchanged` — same severity.
- `progress_pct = resolved / from.len()`.

**Output:**

- Text: bucket sections, progress %.
- JSON: `DiffReport { from_ref, from_sha, buckets, workspace? }`.

**Exit:** 0 success; **2** on LOC threshold; otherwise error.

---

## `heal mark-fixed`

```
heal mark-fixed --finding-id <ID> --commit-sha <SHA> [--json]
```

**Hidden** in `--help` (`#[command(hide = true)]`). Called by
`/heal-code-patch` skill.

Constructs `FixedFinding { finding_id, commit_sha, fixed_at: Utc::now()
}` → `upsert_fixed` → atomic rewrite of `.heal/findings/fixed.json`
(`BTreeMap<finding_id, FixedFinding>`).

On the next `heal status --refresh`, `reconcile_fixed` checks the
finding id; if the finding re-appears, the entry moves to
`regressed.jsonl`; if not, it's silently retired when the user solves
the surrounding finding population.

**Output (text):** `marked <id> as fixed by <sha> (recorded in <path>)`.
**Output (JSON):** `{ finding_id, commit_sha, fixed_at, path }`.

---

## `heal calibrate`

```
heal calibrate [--force] [--json]
```

`commands/calibrate.rs`:

1. Load config — fail if missing (`heal init` must run first).
2. If `calibration.toml` exists AND not `--force` → emit status, return.
3. Else `run_all` → `build_calibration` → save with new `created_at`,
   `calibrated_at_sha`, `codebase_files` metadata.

**Auto-recalibration is forbidden.** HEAL never triggers `heal calibrate`
on its own — `heal-config` skill or the user decides. The header comment
written into `calibration.toml` reflects this.

**Output (text):** path, `codebase_files`, percentile breaks (CCN/Cog
p95, hotspot p90).

**Output (JSON):** `CalibrateReport { kind: "recalibrated" | "ok" |
"missing", calibration? }`.

**Hand-edits to `calibration.toml`:** preserved on read but **overwritten
by `--force`**. Put `floor_*` overrides in `config.toml` instead so they
survive recalibration.

---

## `heal skills <action>`

```
heal skills install [--force] [--json]
heal skills update  [--force] [--json]
heal skills status  [--json]
heal skills uninstall       [--json]
```

`commands/skills.rs`. See `skills-and-hooks.md` for the underlying
`skill_assets` and `claude_settings` mechanisms.

### `install`

Extracts bundled skills from the `include_dir!`-embedded tree to
`<project>/.claude/skills/<skill-name>/`. Modes:

- default → `ExtractMode::InstallSafe` (skip existing files).
- `--force` → `ExtractMode::InstallForce` (overwrite all).

Always sweeps legacy hook entries via `claude_settings::wire`.

### `update`

`ExtractMode::Update { force }`. Drift-aware: skips files where
`canonical(on-disk) != bundled` unless `--force`.

### `status`

Reads SKILL.md `metadata:` block for installed `heal-version`. Compares
to `bundled_version()`. Returns drift list (files user edited).
States: `NotInstalled` | `Installed` (with version comparison:
`up_to_date` | `bundled-newer` | `installed-newer`).

### `uninstall`

Removes bundled skill directories. Sweeps the **pre-v0.2 plugin
layout** (`.claude/plugins/heal/`, `.claude-plugin/marketplace.json`,
`extraKnownMarketplaces["heal-local"]`, `enabledPlugins["heal@heal-local"]`).
Leaves non-bundled sibling skills intact.

---

## Exit codes (full table)

| Code | Meaning | Triggered by |
|---|---|---|
| 0 | success | normal happy path; broken pipe on `heal status` pager |
| 1 | unspecified error | any anyhow `Err(_)` propagation |
| 2 | LOC threshold exceeded | `heal diff` only, when project LOC > `[diff].max_loc_threshold` |

No other documented exit codes. Don't invent new ones without updating
this table and `docs/cli.md`.

---

## Paging contract

Pager only used by `heal status` (the long, read-mostly output). Other
commands write to stdout directly.

Conditions for paging:

```
stdout is TTY  &&  !--json  &&  !--no-pager
```

`spawn_pager()` sets `LESS=FRX`. ANSI is preserved through the pager.
SIGPIPE on user quit returns 0.

---

## Output format toggles

Every public command supports `--json` for machine consumption. Skills
parse JSON; humans parse text. Don't add a third format.

Stable JSON shapes (skills depend on these):

- `heal status --json` → `FindingsRecord` (top-level on-disk shape).
- `heal init --json` → `InitReport`.
- `heal calibrate --json` → `CalibrateReport`.
- `heal diff --json` → `DiffReport`.
- `heal metrics --json` → `{ initialized, ... }` map.
- `heal skills <action> --json` → action-specific structured summary.
- `heal mark-fixed --json` → `{ finding_id, commit_sha, fixed_at, path }`.

Bumping a JSON field name is a **breaking change**. Update consuming
skills in the same PR.

---

## Routing

```rust
// cli.rs (Cli::run dispatch — abridged)
match self.command {
    Init { force, yes, no_skills, json } => commands::init::run(...),
    Hook { event }                       => commands::hook::run(...),
    Metrics { json, metric, workspace }  => commands::metrics::run(...),
    Status(args)                         => commands::status::run(...),
    Diff(args)                           => commands::diff::run(...),
    MarkFixed { ... }                    => commands::mark_fixed::run(...),
    Skills { action }                    => commands::skills::run(...),
    Calibrate { force, json }            => commands::calibrate::run(...),
}
```

Each handler is a thin glue layer over the orchestrator + render path.
