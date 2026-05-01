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

| Command          | Purpose                                                                                 |
| ---------------- | --------------------------------------------------------------------------------------- |
| `heal init`      | Set up `.heal/`, calibrate, and install the post-commit hook in the current repository. |
| `heal skills`    | Install / update / inspect / remove the bundled Claude plugin.                          |
| `heal check`     | Render the cached `CheckRecord` from `.heal/checks/latest.json` (or refresh it).        |
| `heal status`    | Per-metric summary plus the delta since the previous snapshot.                          |
| `heal calibrate` | Recalibrate codebase-relative Severity thresholds.                                      |
| `heal logs`      | Browse the raw hook event log (`.heal/logs/`).                                          |
| `heal snapshots` | Browse the metric / calibration event timeline (`.heal/snapshots/`).                    |
| `heal checks`    | Newest-first list of every `CheckRecord` ever written to `.heal/checks/`.               |
| `heal fix`       | Per-record / per-finding ops on `.heal/checks/` — `show <id>`, `diff`, `mark`.          |
| `heal compact`   | Gzip aged event-log segments; delete the very oldest. Idempotent; safe to run by hand.  |

## Automation commands

Invoked automatically by the git post-commit hook and the Claude
plugin. You do not normally call them by hand.

| Command     | Called by                 | Purpose                                                              |
| ----------- | ------------------------- | -------------------------------------------------------------------- |
| `heal hook` | git and the Claude plugin | Run observers, write snapshots, emit the post-commit Severity nudge. |

---

## `heal init`

Bootstraps heal inside a git repository:

```sh
heal init                # interactive — prompts to install Claude skills
heal init --yes          # also extract the Claude plugin (no prompt)
heal init --no-skills    # skip the plugin entirely (CI / non-Claude users)
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
   Claude plugin into `.claude/plugins/heal/`. The prompt defaults to
   `Y`; pass `--yes` to skip the prompt and install, or `--no-skills`
   to skip without prompting. When `claude` is **not** on `PATH`, the
   prompt is suppressed silently (the plugin would have nothing to
   talk to).

When done, `heal init` prints an "Installed:" summary listing every
file it wrote — config, calibration, initial snapshot, post-commit
hook, and the Claude plugin path (or the reason it was skipped).

Re-running is safe: `config.toml` is left in place unless `--force` is
passed; the post-commit hook is replaced only when it carries the heal
marker. If a non-heal `post-commit` hook already exists, `heal init`
leaves it alone — pass `--force` to overwrite.

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

- one read-only skill `heal-code-review` that ingests `heal check --all
--json`, deep-reads the flagged code, and produces an architectural
  reading plus a prioritised refactor TODO list (with reference docs
  under `skills/heal-code-review/references/`).
- one write skill `heal-code-patch` that drains
  `.heal/checks/latest.json` one finding per commit (Severity order;
  `Critical 🔥` first).

See [Claude plugin](/heal/claude-plugin/) for the full skill contracts.

## `heal check`

Runs every observer, classifies each Finding by Severity using
`calibration.toml`, and writes the result to `.heal/checks/latest.json`
(the TODO list `/heal-code-review` audits and `/heal-code-patch` drains):

```sh
heal check                              # render the cached record (default)
heal check --refresh                    # re-scan and overwrite latest.json
heal check --metric lcom                # only LCOM findings
heal check --severity critical          # only Critical (and above with --all)
heal check --feature src/payments       # restrict to one path prefix
heal check --all                        # show Medium / Ok plus the low-Severity hotspot section
heal check --top 5                      # cap each Severity bucket at 5 rows
heal check --json                       # CheckRecord shape on stdout
```

By default `heal check` is a read-only render of `.heal/checks/latest.json`:
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
| `.heal/checks/`    | `CheckRecord` history written by `heal check`.                            | `heal checks`    |

`heal status` is the synthesised view over snapshots; `heal snapshots`
is the raw timeline. The pre-v0.2 `session-start` event was retired
along with the SessionStart nudge.

## `heal fix`

Per-record / per-finding operations on `.heal/checks/`. Browsing lives
under `heal checks`; `heal fix` is the verb for "I'm working through
the TODO list":

```sh
heal fix show <check_id>              # render one record
heal fix show <check_id> --json       # stable shape (same as `heal check --json`)

heal fix diff                         # latest cache vs a live in-memory scan
heal fix diff <from>                  # <from> vs a live scan
heal fix diff <from> <to>             # two cached records, no scan
heal fix diff --all --json            # show Improved/Unchanged + JSON

heal fix mark --finding-id <id> --commit-sha <sha>   # used by /heal-code-patch
```

The argument shape mirrors `git diff`: omitting `<to>` means "compare
against a fresh in-memory scan of the working tree" (never persisted),
and omitting `<from>` defaults to the most recent cached record. After
a `vs live` diff, if every finding in the FROM record has been logged
to `fixed.jsonl`, the renderer prints a one-line nudge to run `heal
check --refresh` so reconciliation moves resolved entries out (or
surfaces regressions).

`heal check` is the single writer of `.heal/checks/<segment>.jsonl`
(scan results) and `latest.json` (atomic mirror). `heal fix mark` is
the only other writer; it appends a single `FixedFinding` line to
`fixed.jsonl`. `heal fix show` / `heal fix diff` and `heal checks`
are pure readers.

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
- **`heal fix diff`** (no args) lets you verify progress mid-session
  by comparing the latest cached record against a live scan, without
  polluting `.heal/checks/` with extra records.
- **Preserve the post-commit hook.** Removing it stops new snapshots
  from being recorded, and `heal status` / `heal checks` will keep
  showing the previous delta.
