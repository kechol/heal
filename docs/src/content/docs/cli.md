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
| `heal status`    | Render the cached `CheckRecord` from `.heal/findings/latest.json` (or refresh it). The "current TODO". |
| `heal diff`      | Diff the live worktree against a cached `CheckRecord` (default git ref: `HEAD`). Like `git diff`.      |
| `heal metrics`   | Per-metric summary recomputed from the current worktree on every invocation.                           |
| `heal calibrate` | Recalibrate codebase-relative Severity thresholds.                                                     |

## Automation commands

Invoked automatically by the git post-commit hook or by the
`/heal-code-patch` skill. You do not normally call these by hand.
Hidden from `--help`.

| Command           | Called by                | Purpose                                                                                  |
| ----------------- | ------------------------ | ---------------------------------------------------------------------------------------- |
| `heal hook`       | git post-commit          | Run observers and emit the Severity nudge.                                               |
| `heal mark-fixed` | `/heal-code-patch` skill | Record a `FixedFinding` in `.heal/findings/fixed.json` after a fix-per-commit lands.     |

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
   `findings/`, and a `.gitignore` that excludes `findings/` and
   `skills-install.json` (so `config.toml` and `calibration.toml` stay
   tracked and teammates share the same Severity ladder).
2. Run every observer once and compute the codebase's percentile
   distribution per metric — that becomes `calibration.toml` (with a
   provenance comment header pointing at `heal calibrate --force`).
3. Install `.git/hooks/post-commit` (idempotent — the script is marked
   with a comment so re-installation never duplicates the line).
4. If the `claude` CLI is on `PATH`, prompt to extract the bundled
   skill set into `.claude/skills/`. The prompt defaults to `Y`; pass
   `--yes` to skip the prompt and install, or `--no-skills` to skip
   without prompting. When `claude` is **not** on `PATH`, the prompt
   is suppressed silently (the skills would have nothing to talk to).
5. Sweep any legacy `heal hook edit` / `heal hook stop` entries from
   `.claude/settings.json` if present (HEAL no longer registers
   PostToolUse / Stop hooks).

When done, `heal init` prints an "Installed:" summary listing every
file it wrote — config, calibration, post-commit hook, and the
Claude skills path (or the reason it was skipped).

Re-running is safe: `config.toml` is left in place unless `--force` is
passed; the post-commit hook is replaced only when it carries the heal
marker. If a non-heal `post-commit` hook already exists, `heal init`
leaves it alone — pass `--force` to overwrite.

## `heal skills`

Manages the bundled Claude skill set under `.claude/skills/`:

```sh
heal skills install     # extract the skills (run once per repo)
heal skills update      # refresh after upgrading the heal binary
heal skills status      # compare installed vs. bundled
heal skills uninstall   # remove the skills and the manifest
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
- `/heal-code-patch` (write) drains `.heal/findings/latest.json` one
  finding per commit (Severity order; `Critical 🔥` first).
- `/heal-cli` is a concise reference for the `heal` CLI surface.
- `/heal-config` calibrates the project, asks for a strictness level,
  and writes `config.toml` accordingly. It also detects calibration
  drift idempotently from `calibration.toml` meta fields and the
  current `latest.json` / `fixed.json`.

`heal skills install` (and `heal init`) sweep legacy
`heal hook edit` / `heal hook stop` entries out of
`.claude/settings.json` — HEAL no longer registers any Claude hooks.
`uninstall` also sweeps the older plugin/marketplace install layout
(old `.claude/plugins/heal/`, `.claude-plugin/marketplace.json`, and
the `heal@heal-local` settings entries) so users upgrading from an
older heal can land cleanly with one uninstall + reinstall.

See [Claude skills](/heal/claude-skills/) for the full skill contracts.

## `heal status`

Runs every observer, classifies each Finding by Severity using
`calibration.toml`, and writes the result to `.heal/findings/latest.json`
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

By default `heal status` is a read-only render of `.heal/findings/latest.json`:
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
complex functions, top hotspots, most-split classes. `--metric
<name>` scopes output to one observer; valid names: `loc`,
`complexity`, `churn`, `change-coupling`, `duplication`, `hotspot`,
`lcom`. `--json` produces the same data as machine-readable JSON,
suitable for piping into `jq`.

`heal metrics` recomputes everything from scratch on every invocation —
there is no historical record stream, so there is no delta vs. a
prior snapshot.

## `heal calibrate`

```sh
heal calibrate            # create calibration.toml if missing; otherwise no-op
heal calibrate --force    # always rescan and overwrite calibration.toml
```

heal **never** recalibrates automatically.

- When `.heal/calibration.toml` is **missing**, `heal calibrate`
  rescans and writes it. (Normally this only happens before
  `heal init` has run; `init` populates the file as part of bootstrap.)
- When the file **exists**, the default invocation reports the file
  is present and surfaces `--force` as the way to refresh.

Drift detection no longer lives in the CLI. The `/heal-config` skill
takes over by reading `calibration.toml.meta.calibrated_at_sha` /
`calibrated_at_files` against the current `.heal/findings/latest.json`
and `.heal/findings/fixed.json`, and recommends
`heal calibrate --force` when warranted.

The generated `calibration.toml` carries a comment header noting its
provenance and the regeneration command, so anyone opening the file
can find their way back to this command without reading the docs. Put
`floor_critical` and `floor_ok` overrides in `config.toml` so they
survive `heal calibrate --force`.

## Inspecting the cache

`.heal/findings/` is the only on-disk surface, and it holds three
flat artefacts:

| File                            | Shape                            | Purpose                                                                  |
| ------------------------------- | -------------------------------- | ------------------------------------------------------------------------ |
| `.heal/findings/latest.json`    | `CheckRecord` (single object)    | The current TODO list — refreshed by `heal status --refresh`.            |
| `.heal/findings/fixed.json`     | `BTreeMap<finding_id, FixedFinding>` | Bounded record of fixes claimed by `heal mark-fixed`.                |
| `.heal/findings/regressed.jsonl`| append-only JSON-lines           | Audit trail for fixes that were re-detected.                             |

These are plain files, readable with `jq`:

```sh
jq '.severity_counts' .heal/findings/latest.json
jq 'keys | length' .heal/findings/fixed.json     # number of recorded fixes
tail .heal/findings/regressed.jsonl
```

There is no event log, no historical record stream, and no
`heal logs` / `heal snapshots` / `heal checks` / `heal compact`
browser command — those were removed when the event log was retired.

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

`<git-ref>` accepts anything `git rev-parse` understands. When the ref
matches the `head_sha` recorded in `.heal/findings/latest.json`, the
diff runs in place against that cached record. Otherwise heal falls
back to a `git worktree`-backed scan: the requested ref is materialised
into a temporary worktree, observers run there, and the result is
compared to the live worktree.

The worktree-backed mode is gated by `[diff].max_loc_threshold` in
`config.toml` (default `200_000` LOC). Above the threshold,
`heal diff` exits with code 2 and prints a manual two-branch recipe
instead of materialising the worktree.

The right-hand side is **always a fresh in-memory scan** of the
current worktree — never persisted. Output buckets: Resolved /
Regressed / Improved / New / Unchanged, plus a progress percentage.

`heal status` is the single writer of `.heal/findings/latest.json`;
`heal mark-fixed` is the only other writer (it adds an entry to
`fixed.json`, and may move it to `regressed.jsonl` on re-detection).
`heal diff` is a pure reader.

---

## `heal hook` (automation)

Invoked automatically by the git post-commit hook. Manual invocation
is occasionally useful for debugging:

```sh
heal hook commit          # post-commit: run observers, surface nudge
heal hook edit            # legacy no-op (kept for back-compat)
heal hook stop            # legacy no-op (kept for back-compat)
```

`heal hook commit` runs every observer and prints a one-line
Severity nudge — every `Critical` and `High` finding to stdout
(`Medium` and `Ok` stay quiet). Hotspot-flagged entries lead. There
is no cool-down: the same problem reappears every commit until it's
fixed — that's the point. Nothing is written to disk; the nudge is
the only output.

`heal hook edit` and `heal hook stop` are silent no-ops kept only so
stale `.claude/settings.json` registrations from older heal versions
don't error. New installs do not register them.

---

## Migrating from older heal

Existing repositories will have stale state directories that the
current heal no longer creates or reads. They are safe to remove by
hand:

```sh
rm -rf .heal/snapshots .heal/logs .heal/docs .heal/reports .heal/checks
```

`.heal/checks/` may have been renamed to `.heal/findings/` by re-running
`heal init`, but the old directory still lingers — delete it. Re-run
`heal init` once after upgrading so the new `.heal/.gitignore` lands.

---

## Tips

- **`heal status` is the canonical workflow.** After a meaningful
  commit, run it to refresh the cache and see what's still on the
  TODO list.
- **`heal diff`** (no args) shows progress mid-session by comparing
  the live worktree against the cached `CheckRecord` for HEAD —
  useful before committing a fix.
- **Preserve the post-commit hook.** Removing it stops the Severity
  nudge from running after each commit, but `heal status` still works
  on demand.
