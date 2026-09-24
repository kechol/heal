# heal CLI reference

The machine-oriented manual for the `heal` CLI: every subcommand, every
flag that matters for scripting, and the **stable JSON contract** each
`--json` flag emits. Every `/heal:` skill loads this file before it
shells out to `heal`.

Conventions:

- All commands accept a global `--project <PATH>` to operate on a
  directory other than the current one. Omit it inside the repo.
- Commands that take `--json` emit a stable shape. The human-readable
  text is **not** a contract — never parse the prose.
- Paths in `.heal/` are owned by `heal`. Don't hand-edit them except
  `config.toml` (and `concepts.toml` / `doc_pairs.json`, which the
  setup skill writes). Recalibrate with `heal calibrate --force`, never
  by editing `calibration.toml`.

## Output language

Match the user's language for prose. Resolution order:

1. Explicit instruction in the current conversation.
2. The language the user is writing in.
3. `[project].response_language` in `.heal/config.toml` (free-form:
   `"Japanese"`, `"日本語"`, `"ja"`, `"français"` — passed verbatim).
4. English (fallback).

Identifiers stay verbatim — command names (`heal status`), flags
(`--feature docs`), config keys (`[features.docs]`), file paths
(`.heal/findings/latest.json`), `Finding.metric` strings, finding ids,
and JSON field names are part of the contract, not prose. Translate the
surrounding explanation, not the contract. Commit messages follow the
project's own convention, not the chat language.

## The loop

```
heal init                          # one-time: write .heal/, install hook, calibrate
heal doctor --json                 # what is set up, what is missing or stale
heal status --json                 # the TODO list (cached; rescans when stale)
heal mark fix --finding-id … --commit-sha …   # agent-only: after committing a fix
heal mark accept --finding-id … --reason …    # agent-only: record an intrinsic finding
heal calibrate --force             # re-baseline thresholds when the user agrees
```

Behind the scenes:

- A post-commit git hook re-runs every observer, classifies the result
  against `.heal/calibration.toml`, and prints a one-line nudge.
  Failures are swallowed so heal never blocks a commit.
- `heal status` writes its result to `.heal/findings/latest.json`. The
  cache is single-record by design — there is no history. Re-running
  with the same HEAD, clean worktree, config, calibration, and enabled
  observation inputs is a free cache hit; stale state is rescanned
  automatically.
- `.heal/findings/fixed.json` (a map `finding_id → FixedFinding`) and
  `.heal/findings/regressed.jsonl` track the per-finding fix history.
  `.heal/findings/accepted.json` holds findings the team accepted.

## Subcommands (alphabetical)

### `heal auth jev set | status [--offline] | clear [--json]`

Manage the Jev API key outside `.heal/`. `set` reads one line from
stdin into the per-user `credentials.toml` (mode 600);
`TYPESAFE_API_KEY` takes precedence. `status` shows the masked key and
its source, then checks it against the API unless `--offline` (exit 2
when no key is configured or the check fails). `clear` deletes the
stored key. Never ask the user to paste a key into the chat — tell them
to run `heal auth jev set` themselves.

### `heal calibrate [--force] [--json]`

Codebase-relative Severity thresholds. Without `--force`: report the
existing `.heal/calibration.toml` (or create it when missing). With
`--force`: rescan and overwrite. heal never recalibrates on its own;
run `--force` only when the user agrees. JSON:

```jsonc
// kind ∈ {"recalibrated", "ok", "missing"}
{
  "kind": "ok",
  "path": ".heal/calibration.toml",
  "calibration": {
    "meta": { "created_at": "…", "codebase_files": 142, "strategy": "percentile", "calibrated_at_sha": "…" },
    "calibration": { /* per-metric percentile breaks */ }
  }
}
```

Whether a recalibration is due is `heal doctor`'s call (see
`calibration.reasons` there), not this command's.

### `heal diff [<git-ref>] [--all] [--hide-accepted] [--json]`

Diff the current findings against a `FindingsRecord` for the resolved
git ref. Default ref is the calibration baseline SHA, falling back to
`HEAD`: "how much have we drained since calibration?". `<git-ref>`
accepts anything `git rev-parse` understands. Older refs are scanned
in a temporary `git worktree` with the *current* config and
calibration. Gated by `[diff].max_loc_threshold` (default `200_000`
LOC) — over it the command exits 2 with a manual recipe.

Buckets: Resolved / Regressed / Improved / New / Unchanged, plus a
progress percentage. `--all` also shows Improved + Unchanged;
`--hide-accepted` drops accepted entries from the human output (JSON is
never filtered). JSON:

```jsonc
{
  "from_ref":     "HEAD",
  "from_sha":     "deadbeef…",
  "to_head_sha":  "deadbeef…",
  "from_coverage_observation": { "state": "missing", "configured_sources": ["lcov.info"], "sources": [], "unreadable_sources": [], "unmeasured_files": ["src/a.ts"] },
  "to_coverage_observation":   { "state": "complete", "configured_sources": ["lcov.info"], "sources": ["lcov.info"], "unreadable_sources": [], "unmeasured_files": [] },
  "resolved":     [{ "finding_id": "ccn:…", "metric": "ccn", "file": "src/a.ts",
                     "from_severity": "high", "to_severity": null,
                     "from_hotspot": false, "hotspot": false }],
  "regressed":    [],
  "improved":     [],
  "new_findings": [],            // entries carry "accepted": true when accepted
  "unchanged":    [],
  "progress_pct":     0.25,
  "t0_total":         4,
  "t0_resolved":      1,
  "t0_progress_pct":  0.25
}
```

### `heal doctor [--json]`

What is set up and what is missing or stale. Read-only, offline, exit 0
whenever the report is produced. Each section carries
`{ status, detail, fix? }` plus facts; `status` ∈
`ok | todo | warn | off | error` (`off` = an optional family that is
disabled). `todo` lists the sections whose status is `todo` or `error`.

```jsonc
{
  "heal_version": "0.6.0",
  "project": "/path/to/repo",
  "initialized": true,                       // .heal/config.toml exists
  "config": { "status": "ok", "detail": "…", "response_language": "Japanese" },
  "calibration": {
    "status": "todo", "detail": "calibration may be stale (commits_since_calibration)",
    "fix": "heal calibrate --force",
    "created_at": "…", "calibrated_at_sha": "…", "calibrated_at_files": 142,
    "current_files": 150, "commits_since": 240, "fixed_since": 3,
    "reasons": ["commits_since_calibration"] // or file_count_drift / graduated
  },
  "post_commit_hook": { "status": "ok", "detail": "post-commit hook installed" },
  "docs": { "status": "off", "detail": "…", "enabled": false, "pairs_path": ".heal/doc_pairs.json", "missing_paths": 0 },
  "test": { "status": "ok", "detail": "…", "enabled": true, "coverage_enabled": true,
            "lcov_found": ["lcov.info"], "post_commit_refresh": false },
  "semantic": { "status": "todo", "detail": "…", "fix": "/heal:setup", "enabled": true,
                "concepts": 46, "key_source": "environment" },   // or credentials_file; absent without a key
  "legacy_skills": { "status": "ok", "detail": "…", "paths": [] },
  "todo": ["calibration", "semantic"]
}
```

Drift rules behind `calibration.reasons`: `commits_since_calibration`
(more than 200 commits), `file_count_drift` (file count moved by more
than 20%), `graduated` (no Critical / High left and at least 10 fixes
recorded since calibration). They only suggest `heal calibrate --force`;
the user decides.

### `heal hook commit` (internal)

Run by the post-commit git hook `heal init` installs. Not for direct
use; no `--json`. Silently no-ops in a project without `.heal/`.

### `heal init [--force] [--explicit] [--json]`

Set up `.heal/`, write a default `config.toml`, install the post-commit
hook, and scan once. Safe to re-run: an existing `config.toml` and
`calibration.toml` are kept and the hook is refreshed. `--force`
overwrites both and replaces a foreign post-commit hook. `--explicit`
writes every config key including defaults. (`--yes` / `--no-skills`
are accepted for old scripts and do nothing; skills come from the
plugin.)

```jsonc
{
  "project": "/path/to/repo",
  "heal_dir": "…/.heal",
  "primary_language": "rust",
  "config":           { "path": "…/.heal/config.toml",      "action": "kept_existing" },
  "calibration_path": "…/.heal/calibration.toml",
  "calibration":      { "path": "…/.heal/calibration.toml", "action": "kept_existing" },
  "post_commit_hook": { "path": "…/.git/hooks/post-commit", "action": "refreshed" },
  "severity_counts": { "critical": 0, "high": 0, "medium": 0, "ok": 0 },
  "monorepo_signals": [ /* only when manifests suggest undeclared workspaces */ ]
}
```

`config.action` / `calibration.action` ∈ `wrote | overwrote | kept_existing`.
`post_commit_hook.action` ∈ `installed | overwrote | refreshed | skipped_no_repo | skipped_user_hook`.

### `heal mark accept --finding-id <ID> --reason <TEXT> [--json]`

**Agent-only**, hidden from `--help`. Records the team's "won't fix /
acknowledged intrinsic" decision in `.heal/findings/accepted.json`.
Accepted entries persist across re-detections and leave the drain
queue. The finding must be in `latest.json` (run `heal status --refresh`
first if the id came from stale output). Run it only after the user
approved the accept and its reason. JSON:

```jsonc
{
  "finding_id": "ccn:src/a.rs:foo:abc",
  "reason":      "exhaustive_enum_dispatch",
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

### `heal mark fix --finding-id <ID> --commit-sha <SHA> [--json]`

**Agent-only**, hidden from `--help`. After a commit resolves a
finding, records it in `.heal/findings/fixed.json` so the next
`heal status --refresh` either retires it or moves it to
`regressed.jsonl`. One call per finding; a commit that resolves several
findings gets one call for each, all with the same SHA. JSON:
`{ finding_id, commit_sha, fixed_at, path }`.

### `heal metrics [--metric <NAME>] [--feature <code|test|docs>] [--workspace <PATH>] [--json]`

Re-runs every observer and renders per-metric summaries. With
`--json`: one entry per metric, optionally narrowed by `--metric`
(`loc`, `complexity`, `churn`, `change-coupling`, `duplication`,
`hotspot`, `lcom`, and the docs / test metric names) or `--feature`.

### `heal semantic ask [--task <ID>]… [--dry-run] [--refresh] [--prune] [--check] [--focus <FILE>] [--diff <RANGE>] [--json]`

`[features.semantic]` only, and one of the two commands that send
project content over the network (to the TypeSafe Jev API). Plans
questions, skips anything cached, and writes answers to
`.heal/semantic/verdicts/<task>.jsonl` (tracked). On-demand tasks
(`verify_patch`, `verify_proposal`, `verify_tests`, `name_choice`,
`doc_pairs`) and `focus` write to the untracked
`.heal/cache/semantic/verdicts/` and return their answer as
`tasks[].result` in `--json`.

- `--dry-run` — plan and price; sends nothing, needs no key.
- `--task <ID>` — one task (repeatable); required for on-demand tasks.
- `--check` — confirm the key against the API and stop.
- `--focus <FILE>` / `--diff <RANGE>` — the input focus-aware and
  verify tasks judge.

`--json`: `{ model, dry_run, tasks: [{ task, subjects, cached, to_ask,
requests, est_input_tokens, est_usd, answered, failed, oversized_groups,
pruned, hint?, result? }], requests_sent, input_tokens, usd,
stopped_by_budget, fatal, errors }`. Exit 2 when the family is disabled,
no key is configured, the key is rejected, or the API does not know the
configured model — treat that as "skip the semantic step", never as a
failure.

### `heal skills uninstall [--json]`

Removes the skill directories older heal versions copied into the
project (`.claude/skills/heal-*`, `.agents/skills/heal-*`, from a
closed list of names heal itself wrote) and sweeps legacy heal hook
entries from `.claude/settings.json`. Skills now come from the heal
plugin. `heal skills install | update | status` only print that pointer
and exit 1. JSON: `{ "removed": [<project-relative dirs>],
"claude_settings": "updated" | "unchanged" }`.

### `heal status [args] [--json]`

The current TODO list. Reuses fresh cached findings, rescans stale
state automatically, and accepts `--refresh` to force a rescan. Args:

- `--refresh` — rescan and overwrite `.heal/findings/latest.json`.
- `--all` — include Medium and Ok (hidden by default).
- `--severity {critical|high|medium|ok}` — one floor.
- `--metric <NAME>` — one metric. Code: `ccn`, `cognitive`,
  `complexity` (ccn+cognitive), `duplication`, `coupling`
  (`change_coupling` and submetrics), `hotspot`, `lcom`. Docs:
  `doc-freshness`, `doc-drift`, `doc-coverage`, `doc-link-health`,
  `orphan-pages`, `todo-density`, `doc-hotspot`. Test: `coverage-pct`,
  `skip-ratio`, `test-hotspot`. Semantic: `concept` (every
  `concept_*`), `naming` (`term_drift` + `name_mismatch`),
  `test-value`, `mock-scope`, `test-duplicate`, `doc-structure`,
  `doc-placement`, `doc-concept`; `doc-drift` also selects
  `doc_drift.semantic`.
- `--feature {code|test|docs}` — one family. When `test` / `docs` is
  requested and `[features.<f>].enabled = false`, the command exits 1
  with a stderr message naming the switch; check the exit code before
  parsing stdout.
- `--path <PATH-PREFIX>` — findings under a path.
- `--top <N>` — cap each rendered Tier / Severity bucket.
- `--focus <FILE>` (`-` for stdin) — `[features.semantic]` only: order
  each bucket by how much the work described in FILE touches each file,
  from the `focus` verdicts cached by `heal semantic ask --focus FILE`.
  Always rescans and never writes `latest.json`.

JSON: `FindingsRecord` — the shape of `.heal/findings/latest.json` plus
three render-time fields per finding that are never persisted:
`accepted`, `drain_tier`, `drain_rank`.

```jsonc
{
  "version": 9,
  "id": "9f8e7d6c5b4a3210",            // FNV-1a of (head_sha, config_hash, worktree_clean)
  "head_sha": "deadbeef…",
  "worktree_clean": true,
  "config_hash": "…",
  "coverage_observation": { "state": "complete", "configured_sources": ["lcov.info"], "sources": ["lcov.info"], "unreadable_sources": [], "unmeasured_files": [] },
  "severity_counts": { "critical": 3, "high": 11, "medium": 22, "ok": 0 },   // accepted excluded
  "findings": [
    {
      "id": "ccn:src/a.ts:foo:9f8e7d6c5b4a3210",  // stable across runs for the same problem
      "metric": "ccn",
      "severity": "critical",                      // high / medium / ok
      "drain_tier": "must",                        // must (T0) / should (T1) / advisory; absent for Ok or accepted
      "drain_rank": 1,                             // 1 = next in this family's queue
      "hotspot": true,
      "hotspot_score": 140.0,                      // family-local ordering only
      "location":  { "file": "…", "line": 120, "symbol": "…" },
      "locations": [],                             // multi-site findings (duplication / coupling)
      "summary":   "CCN=28",
      "fix_hint":  "Extract input validation",    // only when the observer has one
      "semantic": {                                // [features.semantic] notes; omitted when empty
        "consequence": { "label": "user_facing", "p": 0.71, "confidence": 0.8 }
      }
    }
  ],
  "accepted_rereview": [ /* accepted findings whose Severity or hotspot rose; omitted when empty */ ]
}
```

Optional fields are omitted rather than empty: `locations`, `fix_hint`,
`semantic`, `drain_tier` / `drain_rank`, `accepted` (only `true`),
`is_test_file` (only `true`). `semantic` notes (`consequence`,
`fix_ratio`, `gate`, `effort`, `friction.*`, `split_points`,
`fix_pattern`, `triage_class`, `focus`, …) are optional everywhere: a
teammate without verdicts gets the same findings without them.

`[policy.drain]` sorts drainable findings into tiers: **T0 / must**
(default `critical:hotspot`), **T1 / should** (default `critical`,
`high:hotspot`), **advisory** (the rest above Ok). `drain_rank` already
applies Tier, Severity, the `[features.semantic]` axes, and
`hotspot_score`; never re-derive it or compare ranks across families.

## Exit codes

`heal` exits 1 on internal failure (config parse error, disk write
failure, missing git repo where one is required, a disabled family
requested with `--feature`). It does **not** exit non-zero because
findings exist — gating on Severity is the caller's job. Exit 2 means
something only the user can fix: `heal diff` over
`[diff].max_loc_threshold`, or the semantic setup for
`heal semantic ask` / `heal auth jev status`.
