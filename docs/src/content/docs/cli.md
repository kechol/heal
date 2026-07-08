---
title: CLI
description: The heal subcommand surface, ordered by everyday importance, with examples for daily operations.
---

`heal` is a single binary. Every interaction goes through one of the
subcommands below. Run `heal --help` or `heal <subcommand> --help`
for the full argument list.

## User commands

The day-to-day surface — these are the four you'll actually type.

| Command       | Purpose                                                                                                                |
| ------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `heal init`   | Set up `.heal/`, calibrate, and install the post-commit hook in the current repository.                                |
| `heal skills` | Install / update / inspect / remove the bundled Claude skill set.                                                      |
| `heal status` | Render the current TODO list (or refresh it). Reads `.heal/findings/`.                                                 |
| `heal diff`   | Compare the live worktree against an earlier commit (default: the calibration baseline). Like `git diff` for findings. |

## Automation commands

These run on your behalf — from the git post-commit hook, from a
bundled Claude skill, or only when your codebase has shifted enough
to warrant attention. Hidden from `--help`.

| Command            | Driven by                 | Purpose                                                                       |
| ------------------ | ------------------------- | ----------------------------------------------------------------------------- |
| `heal hook`        | git post-commit           | Run observers and emit the Severity nudge after each commit.                  |
| `heal mark fix`    | `/heal-code-patch` skill  | Record that a commit fixed a finding so the next `heal status` reconciles it. |
| `heal mark accept` | `/heal-code-review` skill | Record an intrinsic finding the team has decided not to refactor.             |
| `heal metrics`     | `/heal-code-review` skill | Per-metric summary recomputed on every invocation.                            |
| `heal calibrate`   | `/heal-setup` skill       | Reset Severity thresholds to today's codebase distribution.                   |

`heal metrics` and `heal calibrate` are listed here because the
bundled skills decide _when_ to run them — `/heal-code-review` reads
the per-metric summary while orchestrating an audit, and
`/heal-setup` watches for calibration drift and recommends a
recalibration when the codebase has moved enough. Run them by hand
only when you need the raw output without going through Claude.

---

## `heal init`

Bootstraps heal inside a git repository:

```sh
heal init                # interactive — one Y/N prompt per detected agent
heal init --yes          # extract skills for every detected agent (no prompt)
heal init --no-skills    # skip every agent (CI / no AI agent installed)
heal init --force        # overwrite an existing config.toml / hook; refresh skills
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
4. For each AI agent on `PATH`, extract the bundled skill set into
   that agent's project-scope discovery path:
   - `claude` → `.claude/skills/`
   - `codex` → `.agents/skills/`

   In a TTY, you get one Y/N prompt per detected agent (default
   `Y`). `--yes` accepts every prompt; `--no-skills` skips every
   prompt. Agents whose CLI is not on `PATH` are silently skipped —
   their skills would have nowhere to be invoked from.

When done, `heal init` prints an "Installed:" summary listing every
file it wrote — config, calibration, post-commit hook, and one line
per agent target (or the reason it was skipped).

Re-running is safe: `config.toml` is left in place unless `--force`
is passed; the post-commit hook is replaced only when it carries
the heal marker. If a non-heal `post-commit` hook already exists,
`heal init` leaves it alone — pass `--force` to overwrite.

## `heal skills`

Manages the bundled skill set across every agent target. Each
subcommand takes `--target <detected|claude|codex|all>` (default
`detected`, mirroring `heal init`):

```sh
heal skills install                  # extract for every CLI on PATH
heal skills install --target codex   # only `.agents/skills/`
heal skills install --target all     # every known target regardless of detection
heal skills update                   # refresh after a heal binary upgrade
heal skills status                   # per-target installed version + drift
heal skills uninstall --target all   # remove from every tree
```

The skill set is embedded in the `heal` binary at compile time, so
each subcommand always operates on the version matching the binary
in use. `update` is drift-aware: files that have been hand-edited
are left in place per target (use `--force` to overwrite anyway).
The Claude target's `install` / `update` also sweep legacy
`heal hook edit` / `heal hook stop` entries from
`.claude/settings.json`; Codex has no sibling settings file.

The bundled set ships eleven skills, grouped by feature family:

**Code (always on):**

- `/heal-code-review` (read-only) ingests `heal status --all --json`,
  deep-reads the flagged code, and produces an architectural reading
  plus a prioritized refactor TODO list.
- `/heal-code-patch` (write) drains the TODO list one finding per
  commit (Severity order; `Critical 🔥` first).
- `/heal-cli` is a concise reference for the `heal` CLI surface.
- `/heal-setup` is the setup wizard. It calibrates the project,
  asks for a strictness level, writes `config.toml`, then asks
  whether to enable the optional `[features.docs]` and
  `[features.test]` families — chaining to `/heal-doc-pair-setup`
  and `/heal-test-reporter-setup` when you opt in. Also detects
  calibration drift and recommends `heal calibrate --force` when
  warranted.

**`[features.docs]`** (opt-in):

- `/heal-doc-pair-setup` (write `.heal/doc_pairs.json`) detects doc
  ⇔ src pairings.
- `/heal-doc-scaffold` (write under `[features.docs] scaffold_root`)
  stands up a fresh documentation tree from codebase signals alone.
- `/heal-doc-review` (read-only) audits the docs slice through a
  Diátaxis lens.
- `/heal-doc-patch` (write) drains broken internal links, dangling
  identifiers, orphan pages, and resolvable TODOs.

**`[features.test]`** (opt-in):

- `/heal-test-reporter-setup` (read-only) proposes lcov reporter +
  CI configuration.
- `/heal-test-review` (read-only) audits the test slice through a
  test-pyramid lens.
- `/heal-test-patch` (write) drains uncovered hot paths, drifting
  tests, and skipped tests whose reason no longer holds.

See [Code › Skills](/heal/code/skills/),
[Test › Skills](/heal/test/skills/), and
[Docs › Skills](/heal/docs/skills/) for the full contracts.

## `heal status`

Runs every observer, classifies each finding by Severity, and writes
the TODO list `/heal-code-review` audits and `/heal-code-patch`
drains:

```sh
heal status                              # render the cached TODO (default)
heal status --refresh                    # re-scan and overwrite the cache
heal status --metric lcom                # only LCOM findings
heal status --metric coverage-pct        # only coverage findings ([features.test])
heal status --metric doc-drift           # only doc-drift findings ([features.docs])
heal status --severity critical          # only Critical (and above with --all)
heal status --feature code               # only the code family (drop test / docs)
heal status --feature test               # only the test family ([features.test])
heal status --feature docs               # only the docs family ([features.docs])
heal status --path src/payments          # restrict to one path prefix (was --feature pre-v0.4)
heal status --all                        # show Medium / Ok plus the low-Severity hotspot section
heal status --top 5                      # cap each Severity bucket at 5 rows
heal status --no-pager                   # write straight to stdout (skip the pager)
heal status --json                       # machine-readable shape on stdout
```

When stdout is a terminal, `heal status` pipes through `$PAGER` (or
`less`) — the same convention as `git diff` / `git log`. Pass
`--no-pager` to write straight to stdout, or pipe the output anywhere
(redirect, `| cat`, CI logs) and the pager is skipped automatically.
`--json` always writes raw to stdout.

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

- A large structural change has shifted the distribution (the
  `/heal-setup` skill watches for this and recommends).
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

| File                             | Purpose                                                       |
| -------------------------------- | ------------------------------------------------------------- |
| `.heal/findings/latest.json`     | The current TODO list — refreshed by `heal status --refresh`. |
| `.heal/findings/fixed.json`      | Bounded record of fixes claimed by `/heal-code-patch`.        |
| `.heal/findings/regressed.jsonl` | Audit trail for fixes that were re-detected.                  |

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
