---
title: CLI
description: The heal subcommand surface, ordered by everyday importance, with examples for daily operations.
---

`heal` is a single binary. Every interaction goes through one of the
subcommands below. Run `heal --help` or `heal <subcommand> --help`
for the full argument list.

## User commands

The day-to-day surface — these are the four you'll actually type.

| Command       | Purpose                                                                                                |
| ------------- | ------------------------------------------------------------------------------------------------------ |
| `heal init`   | Set up `.heal/`, calibrate, and install the post-commit hook in the current repository.                |
| `heal skills` | Install / update / inspect / remove the bundled Claude skill set.                                      |
| `heal status` | Render the current TODO list (or refresh it). Reads `.heal/findings/`.                                 |
| `heal diff`   | Compare the live worktree against an earlier commit (default ref: `HEAD`). Like `git diff` for findings. |

## Automation commands

These run on your behalf — from the git post-commit hook, from a
bundled Claude skill, or only when your codebase has shifted enough
to warrant attention. Hidden from `--help`.

| Command           | Driven by                | Purpose                                                                                  |
| ----------------- | ------------------------ | ---------------------------------------------------------------------------------------- |
| `heal hook`       | git post-commit          | Run observers and emit the Severity nudge after each commit.                             |
| `heal mark-fixed` | `/heal-code-patch` skill | Record that a commit fixed a finding so the next `heal status` reconciles it.            |
| `heal metrics`    | `/heal-code-review` skill | Per-metric summary recomputed on every invocation.                                      |
| `heal calibrate`  | `/heal-config` skill     | Reset Severity thresholds to today's codebase distribution.                              |

`heal metrics` and `heal calibrate` are listed here because the
bundled skills decide *when* to run them — `/heal-code-review` reads
the per-metric summary while orchestrating an audit, and
`/heal-config` watches for calibration drift and recommends a
recalibration when the codebase has moved enough. Run them by hand
only when you need the raw output without going through Claude.

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

1. Create `.heal/` with `config.toml`, `calibration.toml`,
   `findings/`, and a `.gitignore` that excludes `findings/` (so
   `config.toml` and `calibration.toml` stay tracked and teammates
   share the same Severity ladder).
2. Run every observer once and compute the codebase's percentile
   distribution per metric — that becomes `calibration.toml`.
3. Install `.git/hooks/post-commit` (idempotent — re-installation
   never duplicates the line).
4. If the `claude` CLI is on `PATH`, prompt to extract the bundled
   skill set into `.claude/skills/`. The prompt defaults to `Y`; pass
   `--yes` to skip the prompt and install, or `--no-skills` to skip
   without prompting. When `claude` is **not** on `PATH`, the prompt
   is suppressed silently (the skills would have nothing to talk to).

When done, `heal init` prints an "Installed:" summary listing every
file it wrote — config, calibration, post-commit hook, and the
Claude skills path (or the reason it was skipped).

Re-running is safe: `config.toml` is left in place unless `--force`
is passed; the post-commit hook is replaced only when it carries
the heal marker. If a non-heal `post-commit` hook already exists,
`heal init` leaves it alone — pass `--force` to overwrite.

## `heal skills`

Manages the bundled Claude skill set under `.claude/skills/`:

```sh
heal skills install     # extract the skills (run once per repo)
heal skills update      # refresh after upgrading the heal binary
heal skills status      # compare installed vs. bundled
heal skills uninstall   # remove the skills
```

The skill set is embedded in the `heal` binary at compile time, so
`heal skills install` always extracts the version matching the binary
in use. `update` is drift-aware: files that have been hand-edited are
left in place (use `--force` to overwrite anyway).

The bundled set ships four skills:

- `/heal-code-review` (read-only) ingests `heal status --all --json`,
  deep-reads the flagged code, and produces an architectural reading
  plus a prioritised refactor TODO list.
- `/heal-code-patch` (write) drains the TODO list one finding per
  commit (Severity order; `Critical 🔥` first).
- `/heal-cli` is a concise reference for the `heal` CLI surface.
- `/heal-config` calibrates the project, asks for a strictness level,
  and writes `config.toml` accordingly. Also detects calibration
  drift and recommends `heal calibrate --force` when warranted.

See [Claude skills](/heal/claude-skills/) for the full skill contracts.

## `heal status`

Runs every observer, classifies each finding by Severity, and writes
the TODO list `/heal-code-review` audits and `/heal-code-patch`
drains:

```sh
heal status                              # render the cached TODO (default)
heal status --refresh                    # re-scan and overwrite the cache
heal status --metric lcom                # only LCOM findings
heal status --severity critical          # only Critical (and above with --all)
heal status --feature src/payments       # restrict to one path prefix
heal status --all                        # show Medium / Ok plus the low-Severity hotspot section
heal status --top 5                      # cap each Severity bucket at 5 rows
heal status --json                       # machine-readable shape on stdout
```

By default `heal status` is a read-only render of the cached TODO:
runs are free once the cache is warm. Pass `--refresh` to invalidate
and re-run every observer; this is the only path that writes the
cache. A missing cache (e.g. immediately after `heal init`) also
triggers a scan, so the first invocation in a project still works
without flags.

Output groups findings under `🔴 Critical 🔥 / 🔴 Critical / 🟠 High 🔥
/ 🟠 High / 🟡 Medium / ✅ Ok` (last two require `--all`), aggregates
one row per file, and ends with `Goal: 0 Critical, 0 High` plus a
"next steps" line pointing at `claude /heal-code-patch`. With
`--all`, an extra "Ok / Medium 🔥 (low Severity, top-10% hotspot)"
section surfaces files that aren't classified as a problem yet but
get touched often enough to be worth a look.

## `heal diff`

Compare the live worktree against the findings at an earlier commit.
Default ref: `HEAD` ("how does my live worktree compare to the last
commit?"):

```sh
heal diff                              # live vs HEAD
heal diff main                         # live vs main
heal diff v0.2.1                       # live vs the v0.2.1 tag
heal diff HEAD~5                       # live vs 5 commits back
heal diff --all                        # also surface Improved + Unchanged
heal diff --json                       # machine-readable shape
```

`<git-ref>` accepts anything `git rev-parse` understands. heal
re-evaluates the requested ref under the *current* `config.toml` and
`calibration.toml` so the comparison is apples-to-apples — you're
seeing how today's rules judge then-and-now, not the historical
ratings.

Output buckets: Resolved / Regressed / Improved / New / Unchanged,
plus a progress percentage. The right-hand side is always a fresh
in-memory scan of the current worktree — never persisted.

For very large repos the comparison can be expensive; `[diff]` in
`config.toml` exposes a LOC ceiling that switches to a manual
two-branch recipe above the threshold. See
[Configuration › `[diff]`](/heal/configuration/#diff).

## `heal metrics`

```sh
heal metrics
heal metrics --json
heal metrics --metric complexity
heal metrics --metric lcom
```

Prints a summary of every enabled metric — primary language, worst-N
complex functions, top hotspots, most-split classes. `--metric
<name>` scopes output to one observer; valid names: `loc`,
`complexity`, `churn`, `change-coupling`, `duplication`, `hotspot`,
`lcom`. `--json` produces the same data as machine-readable JSON,
suitable for piping into `jq`.

Recomputed from scratch on every invocation — there is no historical
record to delta against.

## `heal calibrate`

```sh
heal calibrate            # create calibration.toml if missing; otherwise no-op
heal calibrate --force    # always rescan and overwrite calibration.toml
```

heal **never** recalibrates automatically — a refactor that genuinely
improves the codebase shouldn't silently move the goalposts. Run
`--force` when:

- A large structural change has shifted the distribution (the
  `/heal-config` skill watches for this and recommends).
- You've changed `floor_critical` / `floor_ok` overrides in
  `config.toml` and want the percentile ladder rebuilt against them.

The generated `calibration.toml` carries a comment header noting its
provenance, so anyone opening the file can find their way back to
this command. Put `floor_critical` / `floor_ok` overrides in
`config.toml`, not `calibration.toml` — that way `heal calibrate
--force` doesn't clobber them.

## Inspecting the cache

`heal status --json` is the contract for scripts. If you want to peek
at the on-disk state directly, three flat files live under
`.heal/findings/`:

| File                            | Purpose                                                                  |
| ------------------------------- | ------------------------------------------------------------------------ |
| `.heal/findings/latest.json`    | The current TODO list — refreshed by `heal status --refresh`.            |
| `.heal/findings/fixed.json`     | Bounded record of fixes claimed by `/heal-code-patch`.                   |
| `.heal/findings/regressed.jsonl`| Audit trail for fixes that were re-detected.                             |

These are plain files, readable with `jq`:

```sh
jq '.severity_counts' .heal/findings/latest.json
jq 'keys | length' .heal/findings/fixed.json     # number of recorded fixes
tail .heal/findings/regressed.jsonl
```

## `heal hook commit`

Invoked automatically by the git post-commit hook installed by
`heal init`. Runs every observer and prints a one-line Severity nudge
— every `Critical` and `High` finding to stdout, with Hotspot-flagged
entries first. There is no cool-down: the same problem reappears
every commit until it's fixed — that's the point. Nothing is written
to disk; the nudge is the only output.

Manual invocation is occasionally useful for debugging:

```sh
heal hook commit
```

## Tips

- **`heal status` is the canonical workflow.** After a meaningful
  commit, run it to refresh the cache and see what's still on the
  TODO list.
- **`heal diff`** (no args) shows progress mid-session by comparing
  the live worktree against HEAD — useful before committing a fix.
- **Preserve the post-commit hook.** Removing it stops the Severity
  nudge from running after each commit, but `heal status` still
  works on demand.
