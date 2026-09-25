---
title: CLI
description: The heal subcommand surface, ordered by everyday importance, with examples for daily operations.
---

`heal` is a single binary. Every interaction goes through one of the
subcommands below. Run `heal --help` or `heal <subcommand> --help`
for the full argument list.

## User commands

The day-to-day surface — these are the ones you'll actually type.

| Command       | Purpose                                                                                                                |
| ------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `heal init`   | Set up `.heal/`, calibrate, and install the post-commit hook in the current repository.                                |
| `heal doctor` | Check which parts of the setup are in place and what is left to do.                                                    |
| `heal status` | Render the current TODO list (or refresh it). Reads `.heal/findings/`.                                                 |
| `heal diff`   | Compare the live worktree against an earlier commit (default: the calibration baseline). Like `git diff` for findings. |

The Claude skills are not part of the CLI — they ship as a Claude Code
plugin (see [Installation](/heal/installation/#claude-code-plugin)).
`heal skills uninstall` removes skill folders that heal 0.6 and earlier
copied into a project.

With the opt-in [Semantic (Jev)](/heal/semantic/) family enabled, two
more commands join them. They are the only heal commands that connect
to the network:

| Command             | Purpose                                                                                     |
| ------------------- | ------------------------------------------------------------------------------------------- |
| `heal semantic ask` | Ask Jev about the code, tests, and docs heal selected, and save the answers under `.heal/`. |
| `heal auth jev`     | Store, check, or remove your Jev API key (`set` / `status` / `clear`).                      |

## Automation commands

These run on your behalf — from the git post-commit hook, from a
Claude skill, or only when your codebase has shifted enough to
warrant attention. `heal hook` and `heal mark` are hidden from
`--help`.

| Command            | Driven by                                     | Purpose                                                                       |
| ------------------ | --------------------------------------------- | ----------------------------------------------------------------------------- |
| `heal hook`        | git post-commit                               | Run observers and emit the Severity nudge after each commit.                  |
| `heal mark fix`    | `/heal:refactor`, `/heal:docs`, `/heal:tests` | Record that a commit fixed a finding so the next `heal status` reconciles it. |
| `heal mark accept` | `/heal:refactor`, `/heal:docs`, `/heal:tests` | Record an intrinsic finding the team has decided not to change.               |
| `heal metrics`     | `/heal:setup`                                 | Per-metric summary recomputed on every invocation.                            |
| `heal calibrate`   | `/heal:setup`                                 | Reset Severity thresholds to today's codebase distribution.                   |

`heal metrics` and `heal calibrate` are listed here because the
skills decide _when_ to run them — `/heal:setup` reads the per-metric
summary while tuning the config, and recommends a recalibration when
`heal doctor` reports that the codebase has moved enough. Run them by
hand only when you need the raw output without going through Claude.

---

## `heal init`

Bootstraps heal inside a git repository:

```sh
heal init                # set up .heal/, calibrate, install the hook
heal init --force        # overwrite an existing config.toml, calibration, and hook
heal init --explicit     # write every default to config.toml (long form)
```

By default, `heal init` writes `config.toml` in **minimal form** —
only fields the user has actually customized appear on disk. A
fresh project is essentially an empty file. `--explicit` writes the
full default tree so the file doubles as a discoverable reference
of every available knob.

`heal init` does:

1. Create `.heal/` with `config.toml`, `calibration.toml`, and
   `findings/`. `config.toml`, `calibration.toml`, and the cache
   under `findings/` are all tracked in git, so teammates on the
   same commit see the same Severity ladder and drain queue.
2. Run every observer once and compute the codebase's percentile
   distribution per metric — that becomes `calibration.toml`.
3. Install `.git/hooks/post-commit` (idempotent — re-installation
   never duplicates the line).

When done, `heal init` prints an "Installed:" summary listing what it
wrote or kept — config, calibration, post-commit hook — and how to add
the Claude Code plugin. It does not install skills itself; `--yes` and
`--no-skills` are still accepted so older scripts keep working, and do
nothing.

Re-running is safe: an existing `config.toml` and `calibration.toml`
are kept unless `--force` is passed, and the post-commit hook is
refreshed only when it carries the heal marker. If a non-heal
`post-commit` hook already exists, `heal init` leaves it alone — pass
`--force` to overwrite. If skill folders from heal 0.6 or earlier are
still in the project, the summary says so.

## `heal doctor`

```sh
heal doctor          # one line per area, with the next step for anything missing
heal doctor --json   # the same report as JSON (what /heal:setup reads)
```

Checks what heal needs and reports each area as `ok`, `todo`,
`warn`, `off` (an optional feature you have not enabled), or
`error`, with the command or skill that fixes it:

- `.heal/config.toml` exists and loads.
- `.heal/calibration.toml` exists and still fits the codebase. It is
  flagged when more than 200 commits landed since calibration, the
  file count moved by more than 20%, or no Critical / High findings
  remain after ten or more recorded fixes. heal never recalibrates by
  itself — run `heal calibrate --force` when you agree.
- The post-commit hook is installed.
- Each optional feature you enabled has what it needs: doc pairs for
  docs, an lcov file for coverage, an API key and a concept list for
  semantic.
- Skill folders that older heal versions copied into the project.

`heal doctor` only reads. It changes nothing and never connects to
the network (use `heal auth jev status` to check the key against the
API). `/heal:setup` starts from this report, which is why re-running
it only does what is missing.

## `heal skills uninstall`

Skills ship as a Claude Code plugin now, so the CLI no longer
installs or updates them. The one subcommand left cleans up after
heal 0.6 and earlier, which copied the skills into each project:

```sh
heal skills uninstall          # remove old heal skill folders
heal skills uninstall --json   # list what was removed as JSON
```

It removes `.claude/skills/heal-*` and `.agents/skills/heal-*`
folders from a fixed list of names heal itself wrote — skills you
wrote yourself stay — and sweeps old heal entries from
`.claude/settings.json`. Commit the deletion afterwards.

`heal skills install`, `update`, and `status` still parse so old
scripts fail with a clear message: they print how to install the
plugin and exit 1.

## `heal status`

Runs every observer, classifies each finding by Severity, and writes
the TODO list the `/heal:refactor`, `/heal:docs`, and `/heal:tests`
skills work from:

```sh
heal status                              # render the cached TODO (default)
heal status --refresh                    # re-scan and overwrite the cache
heal status --metric lcom                # only LCOM findings
heal status --metric coverage-pct        # only coverage findings ([features.test])
heal status --metric doc-drift           # only doc-drift findings ([features.docs])
heal status --severity high              # High and Critical; --all does not lower this floor
heal status --feature code               # only the code family (drop test / docs)
heal status --feature test               # only the test family ([features.test])
heal status --feature docs               # only the docs family ([features.docs])
heal status --path src/payments          # restrict to one path prefix (was --feature pre-v0.4)
heal status --all                        # show Advisory, Medium, Ok, and accepted sections
heal status --top 5                      # cap each Tier/Severity bucket at 5 rows
heal status --no-pager                   # write straight to stdout (skip the pager)
heal status --json                       # machine-readable shape on stdout
```

When stdout is a terminal, `heal status` pipes through `$PAGER` (or
`less`) — the same convention as `git diff` / `git log`. Pass
`--no-pager` to write straight to stdout, or pipe the output anywhere
(redirect, `| cat`, CI logs) and the pager is skipped automatically.
`--json` always writes raw to stdout.

By default `heal status` reuses a fresh cached TODO, so warm runs are
effectively free. A missing or stale cache triggers a scan and writes
the replacement automatically; `--refresh` forces that same rescan and
write even when the cache is fresh.

Freshness includes enabled non-git observations as well as HEAD and
the clean-worktree gate. Updating an ignored LCOV report or doc-pair
file invalidates the cache even when HEAD did not move; mtimes and
absolute checkout paths do not. Coverage provenance in human and JSON
output distinguishes `missing`, `read_error`, `partial`, and
`complete`. Unlisted production files are unmeasured and prompt a
reporter/package-scope check; they are not treated as measured 0%.

Output groups findings by effective Drain Tier and Severity (lower
priority sections require `--all`) and aggregates one row per file.
Hotspot remains visible as `🔥` on an all-hot section or mixed row.
Priority is Tier, Severity, then descending
family-local `hotspot_score`, with metric/path/id tie-breakers. Code,
Test, and Docs scores are never compared with one another, and the
score is not a probability or guaranteed payoff.

`--severity` is always a minimum floor. `--all` can reveal otherwise-hidden
sections at or above that floor, but it never restores findings below it.

## `heal diff`

Compare the live worktree against the findings at an earlier commit.
Default ref: the calibration baseline SHA (recorded by `heal init` /
`heal calibrate --force`), falling back to `HEAD` when no baseline is
recorded — so "Progress: N% complete" reads naturally as "drained
since calibration":

```sh
heal diff                              # live vs the calibration baseline
heal diff HEAD                         # live vs the last commit
heal diff main                         # live vs main
heal diff v0.2.1                       # live vs the v0.2.1 tag
heal diff HEAD~5                       # live vs 5 commits back
heal diff --all                        # also surface Improved + Unchanged + below-High entries
heal diff --hide-accepted              # drop rows already accepted via `heal mark accept`
heal diff --no-pager                   # write straight to stdout (skip the pager)
heal diff --json                       # machine-readable shape
```

`<git-ref>` accepts anything `git rev-parse` understands. heal
re-evaluates the requested ref under the _current_ `config.toml` and
`calibration.toml` so the comparison is apples-to-apples — you're
seeing how today's rules judge then-and-now, not the historical
ratings.

When stdout is a terminal, `heal diff` pipes through `$PAGER` (or
`less`) — same convention as `heal status`. Pass `--no-pager` to write
straight to stdout; `--json` always writes raw to stdout.

Output buckets: Resolved / Regressed / Improved / New / Unchanged,
plus a progress percentage. The right-hand side is always a fresh
in-memory scan of the current worktree — never persisted.

By default the human renderer hides entries whose `from` and `to`
Severity both sit below High (a noisy baseline drowns the actionable
rows otherwise) and prints a `[N entries below High hidden — pass
--all]` footer. `--all` bypasses the filter alongside surfacing the
Improved / Unchanged buckets. The `--json` payload is unfiltered
either way — skills and CI keep seeing every row.

Findings the team has acknowledged via `heal mark accept` render
with a `📌 accepted` marker, so a New or Regressed row reads as
"known, not actionable" at a glance. Pass `--hide-accepted` to drop
those rows entirely and see only the actionable view; a `[N accepted
entries hidden]` footer keeps the count visible. The two filters are
independent — `--all --hide-accepted` shows every severity but still
skips accepted rows.

When coverage is enabled, JSON includes `from_coverage_observation` and
`to_coverage_observation`. Their `missing` / `read_error` / `partial` /
`complete` states and source lists keep an unmeasured side distinct from
measured 0% or 100% coverage.

An accepted finding whose Severity rises or whose family Hotspot turns
from false to true produces a re-review notice in status, the current
side of diff, and the post-commit hook. JSON returns one
`accepted_rereview` entry with one or both reasons. This does not remove
acceptance or return the finding to the drain queue.

For very large repos the comparison can be expensive; `[diff]` in
`config.toml` exposes a LOC ceiling that switches to a manual
two-branch recipe above the threshold. See
[Code › Configuration](/heal/code/configuration/#diff).

## `heal metrics`

```sh
heal metrics
heal metrics --json
heal metrics --metric complexity
heal metrics --metric lcom
heal metrics --metric coverage-pct
heal metrics --metric doc-freshness
heal metrics --no-pager
```

Prints a summary of every enabled metric — primary language, worst-N
complex functions, top hotspots, most-split classes. `--metric
<name>` scopes output to one observer; valid names:

- **Code** (always available): `loc`, `complexity`, `churn`,
  `change-coupling`, `duplication`, `hotspot`, `lcom`.
- **`[features.docs]`** (when enabled): `doc-freshness`,
  `doc-drift`, `doc-coverage`, `doc-link-health`, `orphan-pages`,
  `todo-density`, `doc-hotspot`.
- **`[features.test]`** (when enabled): `coverage-pct`,
  `skip-ratio`, `test-hotspot`.

`--json` produces the same data as machine-readable JSON, suitable
for piping into `jq`.

When stdout is a terminal, `heal metrics` pipes through `$PAGER` (or
`less`) — same convention as `heal status` / `heal diff`. Pass
`--no-pager` to write straight to stdout.

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

- A large structural change has shifted the distribution
  (`heal doctor` flags this, and `/heal:setup` recommends it).
- You've changed `floor_critical` / `floor_ok` overrides in
  `config.toml` and want the percentile ladder rebuilt against them.

The generated `calibration.toml` carries a comment header noting its
provenance, so anyone opening the file can find their way back to
this command. Put `floor_critical` / `floor_ok` overrides in
`config.toml`, not `calibration.toml` — that way `heal calibrate
--force` doesn't clobber them.

With `[features.semantic]` enabled, `heal status --focus <file>`
ranks for the work described in the file (see
[Semantic (Jev)](/heal/semantic/)). It always rescans and does not
update the saved TODO list.

## `heal semantic ask`

Only available when `[features.semantic] enabled = true`. See
[Semantic (Jev)](/heal/semantic/) for what gets sent.

```sh
heal semantic ask --dry-run          # plan and price; nothing is sent
heal semantic ask                    # ask every enabled task
heal semantic ask --task <id>        # one task (repeatable)
heal semantic ask --refresh          # re-ask even where an answer is saved
heal semantic ask --prune            # drop saved answers nothing refers to
heal semantic ask --check            # only check the key against the API
heal semantic ask --task focus --focus plan.md    # rank for the work in plan.md
heal semantic ask --task verify_patch --diff HEAD~1..HEAD   # judge a commit range
heal semantic ask --json             # machine-readable run report
```

Exit code `2` means something only you can fix: the feature is
disabled, no API key is configured, the key was rejected, or the API
does not know the configured `model`.

## `heal auth jev`

```sh
printf '%s\n' "$KEY" | heal auth jev set   # store in your user config (mode 600)
heal auth jev status                        # where the key comes from + a live check
heal auth jev status --offline              # skip the live check
heal auth jev clear                         # remove the stored key
```

`TYPESAFE_API_KEY` (or `TYPESAFEAI_API_KEY`) takes precedence over the
stored key. The key is never written under `.heal/`.

## Inspecting the cache

`heal status --json` is the contract for scripts. If you want to peek
at the on-disk state directly, four flat files live under
`.heal/findings/`:

| File                             | Purpose                                                                                   |
| -------------------------------- | ----------------------------------------------------------------------------------------- |
| `.heal/findings/latest.json`     | Current TODO — reused when fresh; replaced when stale/missing or forced with `--refresh`. |
| `.heal/findings/fixed.json`      | Bounded record of fixes the skills claimed with `heal mark fix`.                          |
| `.heal/findings/accepted.json`   | Findings the team accepted with `heal mark accept` (won't fix / intrinsic).               |
| `.heal/findings/regressed.jsonl` | Audit trail for fixes that were re-detected.                                              |

The two JSON views intentionally are not byte-for-byte identical.
`latest.json` is the raw observer record. `heal status --json` uses the
same record schema, then overlays the current accepted state and the
ephemeral `accepted_rereview` notices, and applies any requested
workspace, feature, metric, path, and Severity filters to findings,
re-review notices, and their aggregate counts. Coverage provenance has
no Severity; it follows workspace/path scope and is omitted when a
non-Test family or non-coverage metric is selected.

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

When `[features.test.coverage]` is enabled and any High / Critical
`coverage_pct` finding sits on a hotspot file, the nudge gains a
second indented line counting "uncovered hotspot" findings — the
shortest possible "the next test should land here" reminder.

When coverage is missing, unreadable, or partial, the hook prints the
same reporter/package-scope guidance as status instead of interpreting
unmeasured files as uncovered Hotspots. It also reports accepted items
whose decision premise now needs re-review.

Manual invocation is occasionally useful for debugging:

```sh
heal hook commit
```

## Tips

- **`heal status` is the canonical workflow.** After a meaningful
  commit, run it to refresh the cache and see what's still on the
  TODO list.
- **`heal diff`** (no args) shows progress against the calibration
  baseline — the "% complete" number reads as "drained since
  calibration". Pass `HEAD` for "since the last commit", or any other
  `git rev-parse`-compatible ref.
- **Preserve the post-commit hook.** Removing it stops the Severity
  nudge from running after each commit, but `heal status` still
  works on demand.
