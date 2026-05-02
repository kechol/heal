---
title: CLI
description: The heal subcommand surface, ordered by everyday importance, with examples for daily operations.
---

`heal` is a single binary. Every interaction goes through one of the
subcommands below. Run `heal --help` or `heal <subcommand> --help`
for the full argument list.

## User commands

Ordered by everyday importance — a typical day involves the top three;
the lower entries are for investigation and maintenance.

| Command          | Purpose                                                                                                |
| ---------------- | ------------------------------------------------------------------------------------------------------ |
| `heal init`      | Set up `.heal/`, calibrate, and install the post-commit hook in the current repository.                |
| `heal skills`    | Install / update / inspect / remove the bundled Claude skill set.                                      |
| `heal status`    | Render the cached `CheckRecord` from `.heal/checks/latest.json` (or refresh it). The "current TODO".   |
| `heal diff`      | Diff the live worktree against a cached `CheckRecord` (default git ref: `HEAD`). Like `git diff`.      |
| `heal metrics`   | Per-metric summary plus the delta since the previous snapshot.                                         |
| `heal calibrate` | Recalibrate codebase-relative Severity thresholds.                                                     |
| `heal logs`      | Browse the raw hook event log (`.heal/logs/`).                                                         |
| `heal snapshots` | Browse the metric / calibration event timeline (`.heal/snapshots/`).                                   |
| `heal checks`    | Newest-first list of every `CheckRecord` ever written to `.heal/checks/`.                              |
| `heal compact`   | Gzip aged event-log segments; delete the very oldest. Idempotent; safe to run by hand.                 |

## Automation commands

Invoked automatically by the git post-commit hook and Claude Code's
`settings.json` hook commands, or by the `/heal-code-patch` skill.
You do not normally call these by hand. Hidden from `--help`.

| Command           | Called by                                       | Purpose                                                                                  |
| ----------------- | ----------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `heal hook`       | git post-commit + Claude `PostToolUse` / `Stop` | Run observers, write snapshots / event log, emit the Severity nudge.                     |
| `heal mark-fixed` | `/heal-code-patch` skill                        | Append a `FixedFinding` to `.heal/checks/fixed.jsonl` after a fix-per-commit lands.      |

---

## `heal init`

Bootstraps heal inside a git repository:

```sh
heal init                # interactive — prompts to install the Claude skills
heal init --yes          # also extract the Claude skills (no prompt)
heal init --no-skills    # skip the skills entirely (CI / non-Claude users)
heal init --force        # overwrite an existing config.toml / hook
```

`heal init` does:

1. Create `.heal/` with `config.toml`, `calibration.toml`, `snapshots/`,
   `logs/`, and `checks/`.
2. Run every observer once and compute the codebase's percentile
   distribution per metric — that becomes `calibration.toml` (with a
   provenance comment header pointing at `heal calibrate --force`).
3. Install `.git/hooks/post-commit` (idempotent — the script is marked
   with a comment so re-installation never duplicates the line).
4. Append the first `MetricsSnapshot` to `.heal/snapshots/`, including
   the Severity tally.
5. If the `claude` CLI is on `PATH`, prompt to extract the bundled
   skill set into `.claude/skills/` and merge HEAL's hook commands
   into `.claude/settings.json`. The prompt defaults to `Y`; pass
   `--yes` to skip the prompt and install, or `--no-skills` to skip
   without prompting. When `claude` is **not** on `PATH`, the prompt
   is suppressed silently (the skills would have nothing to talk to).

When done, `heal init` prints an "Installed:" summary listing every
file it wrote — config, calibration, initial snapshot, post-commit
hook, and the Claude skills path (or the reason it was skipped).

Re-running is safe: `config.toml` is left in place unless `--force` is
passed; the post-commit hook is replaced only when it carries the heal
marker. If a non-heal `post-commit` hook already exists, `heal init`
leaves it alone — pass `--force` to overwrite.

## `heal skills`

Manages the bundled Claude skill set under `.claude/skills/` and the
HEAL hook commands inside `.claude/settings.json`:

```sh
heal skills install     # extract the skills + merge hook commands (run once per repo)
heal skills update      # refresh after upgrading the heal binary
heal skills status      # compare installed vs. bundled
heal skills uninstall   # remove the skills, the manifest, and HEAL's hook commands
```

The skill set is embedded in the `heal` binary at compile time, so
`heal skills install` always extracts the version matching the binary
in use. `update` is drift-aware: files that have been hand-edited are
left in place (use `--force` to overwrite anyway).

The bundled set ships four skills:

- `/heal-code-review` (read-only) ingests `heal status --all --json`,
  deep-reads the flagged code, and produces an architectural reading
  plus a prioritised refactor TODO list (reference docs under
  `.claude/skills/heal-code-review/references/`).
- `/heal-code-patch` (write) drains `.heal/checks/latest.json` one
  finding per commit (Severity order; `Critical 🔥` first).
- `/heal-cli` is a concise reference for the `heal` CLI surface.
- `/heal-config` calibrates the project, asks for a strictness level,
  and writes `config.toml` accordingly.

`uninstall` also sweeps the legacy plugin/marketplace install layout
(old `.claude/plugins/heal/`, `.claude-plugin/marketplace.json`, and
the `heal@heal-local` settings entries) so users upgrading from an
older heal can land cleanly with one uninstall + reinstall.

See [Claude skills](/heal/claude-skills/) for the full skill contracts.

## `heal status`

Runs every observer, classifies each Finding by Severity using
`calibration.toml`, and writes the result to `.heal/checks/latest.json`
(the TODO list `/heal-code-review` audits and `/heal-code-patch` drains):

```sh
heal status                              # render the cached record (default)
heal status --refresh                    # re-scan and overwrite latest.json
heal status --metric lcom                # only LCOM findings
heal status --severity critical          # only Critical (and above with --all)
heal status --feature src/payments       # restrict to one path prefix
heal status --all                        # show Medium / Ok plus the low-Severity hotspot section
heal status --top 5                      # cap each Severity bucket at 5 rows
heal status --json                       # CheckRecord shape on stdout
```

By default `heal status` is a read-only render of `.heal/checks/latest.json`:
runs are free once the cache is warm. Pass `--refresh` to invalidate it
and re-run every observer; this is the only path that writes the cache.
A missing cache (e.g. immediately after `heal init`) also triggers a
scan, so the first invocation in a project still works without flags.

Output groups findings under `🔴 Critical 🔥 / 🔴 Critical / 🟠 High 🔥
/ 🟠 High / 🟡 Medium / ✅ Ok` (last two require `--all`), aggregates
one row per file, and ends with `Goal: 0 Critical, 0 High` plus a
"next steps" line pointing at `claude /heal-code-patch`. With `--all`, an
extra "Ok / Medium 🔥 (low Severity, top-10% hotspot)" section
surfaces files that aren't classified as a problem yet but get touched
often enough to be worth a look.

## `heal metrics`

```sh
heal metrics
heal metrics --json
heal metrics --metric complexity
heal metrics --metric lcom
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

## `heal calibrate`

```sh
heal calibrate            # check drift triggers if calibration.toml exists; create it if not
heal calibrate --force    # always rescan and overwrite calibration.toml
```

heal **never** recalibrates automatically.

- When `.heal/calibration.toml` is **missing**, `heal calibrate`
  rescans and writes it. (Normally this only happens before
  `heal init` has run; `init` populates the file as part of bootstrap.)
- When the file **exists**, the default invocation is read-only: it
  evaluates the drift triggers (90-day age, ±20% codebase file count,
  or 30 days of zero Critical findings) and prints a recommendation,
  surfacing `--force` as the way to refresh.

The post-commit nudge prepends a one-line "consider recalibrating"
hint when the same triggers would fire; the user always decides
whether to invoke `heal calibrate --force`.

The generated `calibration.toml` carries a comment header noting its
provenance and the regeneration command, so anyone opening the file
can find their way back to this command without reading the docs. Put
`floor_critical` and `floor_ok` overrides in `config.toml` so they
survive `heal calibrate --force`.

The calibration audit trail lives in `.heal/snapshots/` as
`event = "calibrate"` records — `heal logs` shows them alongside
commits.

## `heal logs` / `heal snapshots` / `heal checks`

Three sibling browsers over the append-only stores under `.heal/`.
They share the same `--since` / `--limit` / `--json` surface; `heal
logs` and `heal snapshots` additionally accept `--filter <event>`.

```sh
heal logs --filter commit --limit 10        # hook events: commit / edit / stop
heal logs --since 2026-04-01T00:00:00Z

heal snapshots --filter calibrate            # MetricsSnapshot + calibrate events
heal snapshots --json --limit 5

heal checks                                  # newest-first CheckRecord summary
heal checks --json --limit 20                # JSON list of {check_id, started_at, head_sha, severity_counts, …}
```

| Source             | Records                                                                   | Reader command   |
| ------------------ | ------------------------------------------------------------------------- | ---------------- |
| `.heal/logs/`      | `commit` / `edit` / `stop` hook events (lightweight metadata).            | `heal logs`      |
| `.heal/snapshots/` | `commit` (`MetricsSnapshot`) + `calibrate` (`CalibrationEvent`) timeline. | `heal snapshots` |
| `.heal/checks/`    | `CheckRecord` history written by `heal status`.                            | `heal checks`    |

`heal metrics` is the synthesised view over snapshots; `heal snapshots`
is the raw timeline. The pre-v0.2 `session-start` event was retired
along with the SessionStart nudge.

## `heal diff`

Diff the live worktree against a cached `CheckRecord` whose `head_sha`
matches the resolved git ref. Default ref: `HEAD` ("how does my live
worktree compare to the last commit?"):

```sh
heal diff                              # live vs HEAD's cached record
heal diff main                         # live vs main's cached record
heal diff v0.2.1                       # live vs the v0.2.1 tag
heal diff HEAD~5                       # live vs 5 commits back
heal diff --all                        # also surface Improved + Unchanged
heal diff --json                       # stable JSON shape
```

`<git-ref>` accepts anything `git rev-parse` understands. The matching
`CheckRecord` must already exist in `.heal/checks/`; if it doesn't
(e.g. you've never run `heal status` while on that ref), the command
errors with a hint to commit + run `heal status` (or check the ref
out and run `heal status --refresh` first).

The right-hand side is **always a fresh in-memory scan** of the
current worktree — never persisted. Output buckets: Resolved /
Regressed / Improved / New / Unchanged, plus a progress percentage.

`heal status` is the single writer of `.heal/checks/<segment>.jsonl`
(scan results) and `latest.json` (atomic mirror). `heal mark-fixed` is
the only other writer; it appends a single `FixedFinding` line to
`fixed.jsonl`. `heal diff` and `heal checks` are pure readers.

## `heal compact`

```sh
heal compact            # gzip past 90 days, delete past 365; print a one-line summary
heal compact --verbose  # one line per touched file
```

Walks `.heal/{snapshots,logs,checks}/` and applies the retention
policy:

- segments older than **90 days** are gzipped in place
  (`YYYY-MM.jsonl` → `YYYY-MM.jsonl.gz`); readers handle both forms
  transparently.
- segments older than **365 days** are deleted outright.

The same policy runs automatically as part of `heal hook commit`, so
manual invocation is mostly for diagnostics and one-off cleanup —
e.g. after restoring a backup, or to compact a long-quiet repository
without waiting for the next commit. The action is idempotent: the
second run on an already-compacted directory is a no-op.

---

## `heal hook` (automation)

Invoked automatically by the git post-commit hook and Claude Code's
`settings.json` hook commands. Manual invocation is occasionally
useful for debugging:

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

- **`heal status` is the canonical workflow.** After a meaningful
  commit, run it to refresh the cache and see what's still on the
  TODO list.
- **`heal diff`** (no args) shows progress mid-session by comparing
  the live worktree against the cached `CheckRecord` for HEAD —
  useful before committing a fix, without polluting `.heal/checks/`
  with extra records.
- **Preserve the post-commit hook.** Removing it stops new snapshots
  from being recorded, and `heal metrics` / `heal checks` will keep
  showing the previous delta.
