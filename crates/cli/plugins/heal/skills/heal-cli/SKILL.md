---
name: heal-cli
description: Concise, complete reference for the `heal` CLI — every subcommand, flag, and JSON contract an AI coding agent needs to drive HEAL programmatically. Load this when you're about to shell out to `heal` and want the exact command shape, the JSON schema it returns, and the `.heal/` files it reads or writes. Trigger on "how do I run heal …?", "what does `heal metrics --json` return?", "is there a heal command for …?", "/heal-cli".
---

# heal-cli

`heal` is a Rust CLI for code-health monitoring. This skill is the
machine-oriented user manual: every subcommand, every flag that matters
for scripting, and the **stable JSON contract** each `--json` flag
emits.

Conventions used below:

- All commands accept a global `--project <PATH>` to operate on a
  directory other than the current one. Omit it when running from inside
  the repo.
- Every command listed under "Read-only" or "Write" produces JSON when
  invoked with `--json`. The JSON shapes are stable; the human-readable
  text rendering is **not** — never parse the prose.
- Paths in `.heal/` are owned by `heal`. Don't hand-edit them except
  `config.toml` and `calibration.toml` (and even then, prefer
  `heal calibrate --force` over editing thresholds by hand).

## The loop

```
heal init                       # one-time: write .heal/, install hook, calibrate
heal status                      # render the TODO list (cached)
heal status --refresh --json     # rescan + emit machine-readable findings
heal fix mark --finding-id … --commit-sha …    # after committing a fix
heal calibrate --force          # re-baseline thresholds when codebase shifted
```

Behind the scenes:

- A post-commit git hook records a `MetricsSnapshot` to
  `.heal/snapshots/YYYY-MM.jsonl` and a lightweight `commit` event to
  `.heal/logs/YYYY-MM.jsonl`. Failures are swallowed so HEAL never blocks
  a commit.
- `heal status` writes its result to `.heal/checks/latest.json` plus an
  append-only `.heal/checks/YYYY-MM.jsonl` segment. Re-running on the
  same `(head_sha, config_hash, worktree_clean=true)` is a free cache
  hit.
- `fixed.jsonl` and `regressed.jsonl` track the per-finding fix history.

## Subcommands (alphabetical)

### `heal calibrate [--force] [--json]`

Calibrate codebase-relative Severity thresholds. Default: read-only
drift check against `.heal/calibration.toml`. With `--force`: rescan and
overwrite the file. JSON shape:

```jsonc
// kind ∈ {"recalibrated", "ok", "recalibration_recommended", "missing"}
{
  "kind": "ok",
  "path": ".heal/calibration.toml",
  "calibration": { /* full Calibration struct (meta + per-metric breaks) */ },
  "recalibration_check": {
    "fired": false,
    "age_exceeded_days": null,         // i64 when age > 90d
    "file_count_delta_pct": null,      // f64 when |Δ| > 0.20
    "critical_clean_streak_days": null // i64 when ≥ 30d Critical=0
  }
}
```

### `heal status [args] [--json]`

The single source of truth for the current TODO list. Renders cached
findings; pass `--refresh` to rescan first. Useful args:

- `--refresh` — rescan and overwrite `.heal/checks/latest.json`.
- `--all` — surface Medium and Ok tiers (default hides them).
- `--severity {critical|high|medium|ok}` — restrict to one floor.
- `--metric {ccn|cognitive|complexity|duplication|coupling|hotspot|lcom}` —
  restrict to one metric (`complexity` = ccn+cognitive).
- `--feature <PATH-PREFIX>` — restrict to findings under a path.
- `--top <N>` — cap each Severity bucket.

JSON shape: `CheckRecord` — same shape as `.heal/checks/latest.json`.
Key fields:

```jsonc
{
  "check_id": "01HZA…",                      // ULID, lexicographic = chronological
  "started_at": "2026-04-28T09:00:00Z",
  "head_sha": "deadbeef…",
  "worktree_clean": true,
  "config_hash": "…",
  "severity_counts": { "critical": 3, "high": 11, "medium": 22, "ok": 0 },
  "findings": [
    {
      "id": "ccn:src/a.ts:foo:9f8e7d6c5b4a3210",  // deterministic; stable across runs
      "metric": "ccn",
      "severity": "critical",                      // or "high" / "medium" / "ok"
      "hotspot": true,
      "location":  { "file": "…", "line": 120, "symbol": "…" },
      "locations": [],                             // populated for duplication / coupling
      "summary":   "CCN=28",
      "fix_hint":  "Extract input validation"
    }
  ]
}
```

### `heal checks [filters] [--json]`

Browse `.heal/checks/` — newest-first list of every `CheckRecord`
written so far. Filters: `--since <RFC3339>`, `--limit <N>`. JSON: a
JSON array of full records.

### `heal compact [--verbose] [--json]`

Rotate `.heal/{snapshots,logs,checks}/` segments — gzip past 90 days,
delete past 365. Idempotent; the post-commit hook also runs it
best-effort. JSON shape:

```jsonc
{
  "gzipped": 3,
  "deleted": 0,
  "dirs": [
    { "name": "snapshots", "gzipped": [".heal/snapshots/2025-12.jsonl.gz"], "deleted": [] },
    { "name": "logs",      "gzipped": [], "deleted": [] },
    { "name": "checks",    "gzipped": [], "deleted": [] }
  ]
}
```

### `heal fix show <CHECK_ID> [--json]`

Render one historical `CheckRecord`. `--json` is the stable contract;
the human view is unstable.

### `heal fix diff [FROM] [TO] [--all] [--json]`

Compare two `CheckRecord`s and bucket findings into Resolved /
Regressed / Improved / New / Unchanged. Argument shape mirrors
`git diff`:

- no args → latest cache vs **live** scan (not persisted).
- `<FROM>` → that record vs live.
- `<FROM> <TO>` → two cached records.

Pass `--all` to also surface Improved + Unchanged.

### `heal fix mark --finding-id <ID> --commit-sha <SHA> [--json]`

Append a `FixedFinding` to `.heal/checks/fixed.jsonl`. Call after each
fix-per-commit so the next `heal status --refresh` reconciles. JSON:

```jsonc
{
  "action": "marked",
  "finding_id": "ccn:src/a.rs:foo:abc",
  "commit_sha": "deadbeef…",
  "fixed_at": "2026-04-28T09:00:00Z",
  "log": ".heal/checks/fixed.jsonl"
}
```

### `heal init [--force] [--yes|--no-skills] [--json]`

One-time setup: `.heal/` layout, default `config.toml`, post-commit
hook, initial scan + calibration, optional Claude-plugin install.

- `--force` overwrites an existing `config.toml` and refreshes the hook
  (preserving the user-marker check).
- `--yes` / `--no-skills` skip the interactive plugin prompt — pass one
  in non-TTY contexts.
- `--json` emits a typed install report:

```jsonc
{
  "project": "/path/to/repo",
  "heal_dir": "…/.heal",
  "primary_language": "rust",
  "config":       { "path": "…/.heal/config.toml",      "action": "wrote" },
  "calibration_path": "…/.heal/calibration.toml",
  "snapshots_dir":    "…/.heal/snapshots",
  "post_commit_hook": { "path": "…/.git/hooks/post-commit", "action": "installed" },
  "skills": {
    "dest": "…/.claude/skills",
    "action": "installed",                // or declined / suppressed_by_flag / skipped_*
    "added": 42, "updated": 0, "unchanged": 0
  },
  "severity_counts": { "critical": 0, "high": 0, "medium": 0, "ok": 0 }
}
```

`config.action` ∈ `wrote | overwrote | kept_existing`.
`post_commit_hook.action` ∈ `installed | overwrote | refreshed | skipped_no_repo | skipped_user_hook`.
`skills.action` ∈ `installed | declined | suppressed_by_flag | skipped_no_claude | skipped_non_interactive`.

### `heal logs [filters] [--json]`

Browse `.heal/logs/` — raw hook event timeline (`commit`, `edit`,
`stop`). Filters: `--since`, `--filter <event-name>`, `--limit`.

### `heal snapshots [filters] [--json]`

Browse `.heal/snapshots/` — typed metric series. `commit` events carry
`MetricsSnapshot` payloads; `calibrate` events carry `CalibrationEvent`
payloads. Same filter shape as `heal logs`.

### `heal metrics [--metric <NAME>] [--json]`

Synthesised view of the latest `MetricsSnapshot` plus deltas. With
`--json`: stable shape mirroring `MetricsSnapshot`, optionally
restricted to one metric (`loc`, `complexity`, `churn`,
`change-coupling`, `duplication`, `hotspot`, `lcom`).

### `heal skills install [--force] [--json]` / `update [--force] [--json]` / `status [--json]` / `uninstall [--json]`

Manage the bundled skill set under `<project>/.claude/skills/`. Each
top-level child of the embedded tree (`heal-cli`, `heal-config`,
`heal-code-review`, `heal-code-patch`) extracts to a sibling directory
under `.claude/skills/`. The same install merges HEAL's hook commands
into `.claude/settings.json` (PostToolUse → `heal hook edit`, Stop →
`heal hook stop`); uninstall removes only HEAL's command entries.

The drift-detection manifest lives at `.heal/skills-install.json`
(heal-owned state, not under `.claude/`).

`status --json`:

```jsonc
{
  "state": "installed",                   // or not_installed / no_manifest
  "dest": ".claude/skills",
  "installed": "0.2.1",
  "bundled":   "0.2.1",
  "installed_at": "2026-04-28T09:00:00Z",
  "source":    "bundled",
  "version_status": "up_to_date",         // or bundled_newer / installed_newer
  "drift": []                             // relative paths edited since install
}
```

`install --json` / `update --json`:

```jsonc
{
  "action": "installed",                  // or "updated"
  "dest": ".claude/skills",
  "version": "0.2.1",
  "source":  "bundled",
  "files": { "added": 42, "updated": 0, "unchanged": 0, "skipped": 0, "user_modified": 0 },
  "user_modified_paths": [],
  "claude":  { "settings": "created" }    // or updated / unchanged
}
```

`uninstall --json`:

```jsonc
{ "action": "removed", "dest": ".claude/skills", "skills_removed": ["heal-cli", "heal-code-patch", "heal-code-review", "heal-config"] }
// or { "action": "noop", … } when nothing was installed
```

### `heal hook <commit|edit|stop>` (internal)

Hook entrypoint invoked by git (`commit`) and Claude Code's
`settings.json` hook commands (`edit`, `stop`). Not for direct use —
`heal init` / `heal skills install` wire it up. No `--json` (output is
a one-line nudge to the user's terminal, not a programmatic contract).
Silently no-ops when invoked in a project that has no `.heal/`.

## Common patterns

**Wait for a clean check.** Run `heal status --refresh --json | jq
'.severity_counts'`; succeed when `critical = 0` (or whatever your gate
is). The check is idempotent on a clean worktree — re-running is free.

**Programmatically drain T0 findings.** Read
`.heal/checks/latest.json`, filter `findings` by your `[policy.drain]`
spec (default: Critical-with-`hotspot=true`), pick one, fix it, then:

```sh
git commit -m "fix: …"
heal fix mark --finding-id "<id>" --commit-sha "$(git rev-parse HEAD)"
heal status --refresh --json    # re-scan; the finding either disappears or surfaces in regressed.jsonl
```

**Force a fresh scan after policy changes.** Editing `config.toml` or
`calibration.toml` invalidates every cached `CheckRecord`. Re-run with
`--refresh` once.

**CI gating.** `heal status --refresh --json --severity critical` and
fail the build if `severity_counts.critical > 0`. Keep `--all` off so
the tier you're gating on is unambiguous.

## Exit codes

`heal` exits non-zero only on **internal failure** (config parse error,
disk write failure, missing git repo where one is required). It does
**not** exit non-zero when findings exist — gating on Severity is the
caller's job (parse `--json`).

## Where to look next

- `/heal-config` — calibrate + survey the codebase, then write or update
  `.heal/config.toml` with thresholds tuned to a chosen strictness.
- `/heal-code-review` — read the cache as a system; produce an
  architectural reading + prioritised TODO.
- `/heal-code-patch` — drain the TODO list one finding per commit.
