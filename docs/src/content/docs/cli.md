---
title: CLI
description: The heal subcommand surface, with examples for everyday operations.
---

`heal` is a single binary. Every interaction goes through one of the
subcommands below. Run `heal --help` or `heal <subcommand> --help`
for the full argument list.

## User commands

These are the commands you run directly in a terminal.

| Command          | Purpose                                                                                       |
| ---------------- | --------------------------------------------------------------------------------------------- |
| `heal init`      | Set up `.heal/`, calibrate, and install the post-commit hook in the current repository.        |
| `heal status`    | Per-metric summary plus the delta since the previous snapshot.                                |
| `heal logs`      | Stream the raw hook event log.                                                                |
| `heal check`     | Run every observer, classify findings by Severity, and refresh `.heal/checks/latest.json`.    |
| `heal cache`     | Read-only views of the `.heal/checks/` cache (`log` / `show` / `diff`) plus `mark-fixed`.     |
| `heal calibrate` | Recalibrate codebase-relative Severity thresholds.                                            |
| `heal skills`    | Install, update, or remove the bundled Claude plugin.                                         |

## Automation commands

Invoked automatically by the git post-commit hook and the Claude
plugin. You do not normally call them by hand.

| Command     | Called by                 | Purpose                                                                |
| ----------- | ------------------------- | ---------------------------------------------------------------------- |
| `heal hook` | git and the Claude plugin | Run observers, write snapshots, emit the post-commit Severity nudge.   |

---

## `heal init`

Bootstraps heal inside a git repository:

```sh
heal init
```

`heal init` does:

1. Create `.heal/` with `config.toml`, `calibration.toml`, `snapshots/`,
   `logs/`, and `checks/`.
2. Run every observer once and compute the codebase's percentile
   distribution per metric — that becomes `calibration.toml`.
3. Install `.git/hooks/post-commit` (idempotent — the script is marked
   with a comment so re-installation never duplicates the line).
4. Append the first `MetricsSnapshot` to `.heal/snapshots/`, including
   the Severity tally.

Re-running is safe: `config.toml` is left in place unless `--force` is
passed; the post-commit hook is replaced only when it carries the heal
marker. If a non-heal `post-commit` hook already exists, `heal init`
leaves it alone — pass `--force` to overwrite.

## `heal status`

```sh
heal status
heal status --json
heal status --metric complexity
heal status --metric lcom
```

Prints a summary of every enabled metric — primary language, worst-N
complex functions, top hotspots, most-split classes — together with a
delta block showing movement since the previous commit. `--metric
<name>` scopes output to one observer; valid names: `loc`,
`complexity`, `churn`, `change-coupling`, `duplication`, `hotspot`,
`lcom`. `--json` produces the same data as machine-readable JSON,
suitable for piping into `jq`.

If `.heal/snapshots/` is empty (for example, immediately after
`heal init` and before the first commit), the command reports that no
snapshots are available.

## `heal logs`

```sh
heal logs
heal logs --filter commit --limit 10
heal logs --since 2026-04-01T00:00:00Z
heal logs --json
```

Each record is a single JSON line. Three event types are produced
under `.heal/logs/`:

- `commit` — written by the git post-commit hook (sha, parent,
  author, message summary, file/line counts).
- `edit` — written when Claude edits a file (PostToolUse hook).
- `stop` — written when a Claude turn ends (Stop hook).

`heal status` reads `snapshots/` (the heavy metric payloads); `heal
logs` reads `logs/` (the lightweight event timeline). The two are
complementary. The pre-v0.2 `session-start` event was retired along
with the SessionStart nudge.

## `heal check`

Runs every observer, classifies each Finding by Severity using
`calibration.toml`, and writes the result to `.heal/checks/latest.json`
(the TODO list `/heal-fix` reads):

```sh
heal check                              # full Severity-grouped view
heal check --metric lcom                # only LCOM findings
heal check --severity critical          # only Critical (and above with --all)
heal check --feature src/payments       # restrict to one path prefix
heal check --hotspot                    # surface low-Severity hotspot files
heal check --top 5                      # cap each Severity bucket at 5 rows
heal check --json                       # CheckRecord shape on stdout
heal check --since-cache                # re-render the latest cache without scanning
```

Output groups findings under `🔴 Critical 🔥 / 🔴 Critical / 🟠 High 🔥
/ 🟠 High / 🟡 Medium / ✅ Ok` (last two require `--all`), aggregates
one row per file, and ends with `Goal: 0 Critical, 0 High` plus a
"next steps" line pointing at `claude /heal-fix`.

Cache hits short-circuit the heavy scan: the same `head_sha`
+ `config_hash` + clean worktree returns the previous record without
re-running observers.

The pre-v0.2 positional names (`overview` / `hotspots` / `complexity`
/ `duplication` / `coupling`) still work as a deprecation alias —
they print a warning and translate to the appropriate `--metric` /
`--hotspot` flag. They will be removed in v0.3.

## `heal cache`

Read-only inspection of `.heal/checks/`, plus the one mutating
sub-command (`mark-fixed`) used by `/heal-fix`:

```sh
heal cache log                          # newest-first list of CheckRecords
heal cache log --json --limit 20

heal cache show <check_id>              # render one record
heal cache show <check_id> --json       # stable shape

heal cache diff                         # latest two records
heal cache diff <from> <to>             # explicit pair
heal cache diff --worktree              # live tree vs latest cache, no write
heal cache diff --all --json            # show Improved/Unchanged + JSON

heal cache mark-fixed --finding-id <id> --commit-sha <sha>
```

`heal check` is the single writer of `.heal/checks/`. `heal cache *`
never mutates state except for `mark-fixed`, which appends a single
`FixedFinding` line to `fixed.jsonl`.

## `heal calibrate`

```sh
heal calibrate                          # rescan + write a fresh calibration.toml
heal calibrate --reason "annual review" # tag the audit log entry
heal calibrate --check                  # evaluate auto-detect triggers, no write
```

heal **never** recalibrates automatically. The post-commit nudge
prepends a one-line "consider recalibrating" hint when
`heal calibrate --check` would have fired (90-day age, ±20% codebase
file count, or 30 days of zero Critical findings); the user always
decides whether to invoke `heal calibrate`.

The calibration audit trail lives in `.heal/snapshots/` as
`event = "calibrate"` records — `heal logs` shows them alongside
commits.

## `heal skills`

Manages the bundled Claude plugin under `.claude/plugins/heal/`:

```sh
heal skills install     # extract the plugin (run once per repository)
heal skills update      # refresh after upgrading the heal binary
heal skills status      # compare installed vs. bundled
heal skills uninstall   # remove .claude/plugins/heal/
```

The plugin tree is embedded in the `heal` binary at compile time, so
`heal skills install` always extracts the version matching the binary
in use. `update` is drift-aware: files that have been hand-edited are
left in place (use `--force` to overwrite anyway).

The bundled plugin ships:

- five read-only `check-*` skills (`overview` / `hotspots` /
  `complexity` / `duplication` / `coupling`) that pull from
  `heal status --metric <x>`.
- one write skill `heal-fix` that drains `.heal/checks/latest.json`
  one finding per commit (Severity order; `Critical 🔥` first).

---

## `heal hook` (automation)

Invoked automatically by the git post-commit hook and the Claude
plugin. Manual invocation is occasionally useful for debugging:

```sh
heal hook commit          # post-commit: run observers, write a snapshot, surface nudge
heal hook edit            # Claude PostToolUse: log file edit
heal hook stop            # Claude Stop: log turn end
```

The post-commit nudge surfaces every `Critical` and `High` finding to
stdout (`Medium` and `Ok` stay quiet). Hotspot-flagged entries lead.
There is no cool-down: the same problem reappears every commit until
it's fixed — that's the point.

---

## Tips

- **`heal check` is the canonical workflow.** After a meaningful
  commit, run it to refresh the cache and see what's still on the
  TODO list.
- **`heal cache diff --worktree`** lets you verify progress mid-session
  without polluting `.heal/checks/` with extra records.
- **Preserve the post-commit hook.** Removing it stops new snapshots
  from being recorded, and `heal status` / `heal cache log` will keep
  showing the previous delta.
