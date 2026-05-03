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
heal mark fix --finding-id … --commit-sha …    # agent-only: after committing a fix
heal mark accept --finding-id … --reason …     # agent-only: record an intrinsic finding
heal calibrate --force          # re-baseline thresholds when codebase shifted
```

Behind the scenes:

- A post-commit git hook re-runs every observer, classifies the result
  against `.heal/calibration.toml`, and prints a one-line nudge.
  Failures are swallowed so HEAL never blocks a commit. No event log
  is written — `latest.json` (refreshed on `heal status --refresh`) is
  the live state.
- `heal status` writes its result to `.heal/findings/latest.json`. The
  cache is single-record by design — there is no historical stream.
  Re-running on the same `(head_sha, config_hash, worktree_clean=true)`
  is a free cache hit.
- `.heal/findings/fixed.json` (a `BTreeMap<finding_id, FixedFinding>`)
  and `.heal/findings/regressed.jsonl` track the per-finding fix
  history.

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

- `--refresh` — rescan and overwrite `.heal/findings/latest.json`.
- `--all` — surface Medium and Ok tiers (default hides them).
- `--severity {critical|high|medium|ok}` — restrict to one floor.
- `--metric {ccn|cognitive|complexity|duplication|coupling|hotspot|lcom}` —
  restrict to one metric (`complexity` = ccn+cognitive).
- `--feature <PATH-PREFIX>` — restrict to findings under a path.
- `--top <N>` — cap each Severity bucket.

JSON shape: `FindingsRecord` — same shape as `.heal/findings/latest.json`.
Key fields:

```jsonc
{
  "version": 2,
  "id": "01HZA…",                            // ULID, lexicographic = chronological
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

### `heal diff [<git-ref>] [--all] [--json]`

Diff the current findings against a `FindingsRecord` for the resolved
git ref. Default ref is `HEAD`: "how does my live worktree compare to
the last commit?"

`<git-ref>` accepts anything `git rev-parse` understands —
`HEAD`, `main`, `v0.2.1`, `HEAD~3`, or a (partial / full) SHA. If
`.heal/findings/latest.json` already corresponds to the resolved ref
(matching `head_sha`), `heal diff` reads it directly. On a miss it
materialises the source at the ref via `git worktree add --detach`,
runs the observer pipeline there using the *current* `config.toml` /
`calibration.toml` (apples-to-apples), and tears the worktree down on
exit. Gated by `[diff].max_loc_threshold` (default `200_000` LOC) —
over the threshold the command exits with code 2 and prints a manual
two-branch recipe. The right-hand side is always a fresh in-memory
scan of the current worktree (never persisted).

Buckets: Resolved / Regressed / Improved / New / Unchanged, plus a
progress percentage. Pass `--all` to also surface Improved +
Unchanged. JSON shape:

```jsonc
{
  "from_ref": "HEAD",
  "from_sha": "deadbeef…",
  "from_started_at": "2026-04-28T09:00:00Z",
  "to_started_at":   "2026-04-28T09:05:00Z",
  "to_head_sha": "deadbeef…",
  "resolved":     [{ "finding_id": "ccn:…", "metric": "ccn", "file": "src/a.ts",
                     "from_severity": "high", "to_severity": null, "hotspot": false }],
  "regressed":    [],
  "improved":     [],
  "new_findings": [],
  "unchanged":    [],
  "progress_pct": 0.25
}
```

### `heal mark fix --finding-id <ID> --commit-sha <SHA> [--json]`

**Agent-only.** Hidden from the top-level `--help`. Upserts a
`FixedFinding` entry into the `BTreeMap` at `.heal/findings/fixed.json`
after committing a fix so the next `heal status --refresh` either
retires the entry (genuinely fixed) or moves it to `regressed.jsonl`.
JSON:

```jsonc
{
  "finding_id": "ccn:src/a.rs:foo:abc",
  "commit_sha": "deadbeef…",
  "fixed_at": "2026-04-28T09:00:00Z",
  "path": ".heal/findings/fixed.json"
}
```

`heal mark-fixed --finding-id … --commit-sha …` is the deprecated
v0.2 alias. It still works but prints a stderr deprecation warning;
prefer `heal mark fix`.

### `heal mark accept --finding-id <ID> --reason <TEXT> [--json]`

**Agent-only.** Hidden from the top-level `--help`. Records the
team's "won't fix / acknowledged intrinsic" decision into
`.heal/findings/accepted.json`. Distinct from `mark fix` — accepted
entries persist across re-detections by design (the whole point is
that the finding stays put and stops cluttering the drain queue).

The `--reason` flag is optional (defaults to empty string) since
the AI agent driving `/heal-code-review` is expected to fill it.
The finding must be present in `latest.json`; run
`heal status --refresh` first if the id was lifted from a stale
output. The CLI snapshots severity, hotspot, `metric_value`, and
summary into the `AcceptedFinding` so later auditors can see what
the decision was made against.

JSON:

```jsonc
{
  "finding_id": "ccn:src/a.rs:foo:abc",
  "reason":      "intrinsic dispatcher; splitting loses exhaustiveness",
  "file":        "src/a.rs",
  "metric":      "ccn",
  "severity":    "critical",
  "hotspot":     true,
  "metric_value": 28.0,
  "summary":     "CCN=28 foo (rust)",
  "accepted_at": "2026-05-03T12:00:00Z",
  "accepted_by": "Alice <alice@example.com>",
  "path":        ".heal/findings/accepted.json"
}
```

### `heal init [--force] [--yes|--no-skills] [--json]`

One-time setup: `.heal/` layout, default `config.toml`, post-commit
hook, initial scan + calibration, optional Claude-skills install.

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

### `heal metrics [--metric <NAME>] [--json]`

Re-runs every observer and renders the result. With `--json`: stable
shape with one entry per metric, optionally restricted via
`--metric` (`loc`, `complexity`, `churn`, `change-coupling`,
`duplication`, `hotspot`, `lcom`). No historical delta — there is no
event log to compare against.

### `heal skills install [--force] [--json]` / `update [--force] [--json]` / `status [--json]` / `uninstall [--json]`

Manage the bundled skill set under `<project>/.claude/skills/`. Each
top-level child of the embedded tree (`heal-cli`, `heal-config`,
`heal-code-review`, `heal-code-patch`) extracts to a sibling directory
under `.claude/skills/`. HEAL no longer registers any Claude Code
hooks; install/uninstall sweep stale `heal hook edit` / `heal hook
stop` entries from `.claude/settings.json` if present.

There is no sidecar manifest. Each `SKILL.md` carries a `metadata:`
block in its YAML frontmatter (`heal-version`, `heal-source`); drift
detection compares `canonical(on-disk)` (the metadata block stripped)
against the bundled raw bytes.

`status --json`:

```jsonc
{
  "state": "installed",                   // or not_installed
  "dest": ".claude/skills",
  "installed": "0.2.1",                   // omitted on pre-metadata installs
  "bundled":   "0.2.1",
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
`.heal/findings/latest.json`, filter `findings` by your `[policy.drain]`
spec (default: Critical-with-`hotspot=true`), pick one, fix it, then:

```sh
git commit -m "fix: …"
heal mark fix --finding-id "<id>" --commit-sha "$(git rev-parse HEAD)"
heal status --refresh --json    # re-scan; the finding either disappears or surfaces in regressed.jsonl
```

**Force a fresh scan after policy changes.** Editing `config.toml` or
`calibration.toml` invalidates the cached `FindingsRecord` (the
`config_hash` shifts). Re-run with `--refresh` once.

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
  architectural reading + prioritized TODO.
- `/heal-code-patch` — drain the TODO list one finding per commit.
