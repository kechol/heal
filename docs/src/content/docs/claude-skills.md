---
title: Claude skills
description: How heal's bundled Claude Code skills connect heal's metrics to Claude sessions, with the /heal-code-review audit and /heal-code-patch repair loop, plus heal-cli and heal-config helper skills.
---

heal ships with a bundled set of Claude Code skills so the metrics it
collects flow into Claude sessions automatically. They are installed
once per repository with `heal skills install`. From that point on:

- A read-only skill `/heal-code-review` audits
  `.heal/findings/latest.json` and produces an architectural reading
  plus a prioritised refactor TODO list.
- A write skill `/heal-code-patch` drains the same cache one finding
  per commit, in Severity order, until the cache is empty or you
  stop the session.
- Two helper skills, `/heal-cli` and `/heal-config`, give Claude
  reference material for driving the CLI and tuning `config.toml`.

heal does not register any Claude Code hooks — no PostToolUse, no
Stop, no SessionStart. The post-commit hook (run by `heal init`'s
git installation) handles the per-commit signal — see
[Architecture › The big picture](/heal/architecture/#the-big-picture).

## Installing it

```sh
heal skills install
```

This extracts each skill directly under `<project>/.claude/skills/`,
where Claude Code natively discovers project-scope skills:

```
.claude/skills/
├── heal-cli/
│   └── SKILL.md
├── heal-code-patch/
│   └── SKILL.md
├── heal-code-review/
│   ├── SKILL.md
│   └── references/
│       ├── architecture.md
│       ├── metrics.md
│       └── readability.md
└── heal-config/
    ├── SKILL.md
    └── references/
        └── config.md
```

The skill set is embedded in the `heal` binary at compile time, so
the version installed always matches the binary. After upgrading
`heal`, run `heal skills update` to refresh.

## No Claude Code hooks

heal does not register PostToolUse, Stop, or SessionStart hooks in
`.claude/settings.json`. The repair loop runs entirely through the
`/heal-code-patch` skill (below); the per-commit Severity nudge is
handled by the git post-commit hook installed by `heal init`.

`heal skills install` (and `heal init`) sweep legacy
`heal hook edit` / `heal hook stop` entries out of
`.claude/settings.json` if a previous heal version registered them.
Other entries you wrote stay untouched. The `heal hook edit` /
`heal hook stop` subcommands themselves remain as silent no-ops so
stale settings from older heal versions don't surface errors.

## The audit skill: `/heal-code-review`

Read-only. Ingests `heal status --all --json`, deep-reads the flagged
code, and returns two artefacts:

1. An **architectural reading** of the codebase — what the findings
   say _as a system_, not as a list (the dominant axis: complexity,
   duplication, coupling, or hub).
2. A **prioritised TODO list** — drawn from **T0 (`must`) only** by
   default. T1 (`should`) findings get a separate "If bandwidth
   permits" section, and Advisory findings are surfaced as a count
   only. The TODO entries are concrete refactors keyed to specific
   files / functions, each tagged with the established refactor
   pattern and the expected metric movement.

The skill is language-agnostic and tailors proposals to the
codebase's apparent style instead of pushing a one-size-fits-all
template. Three reference files ship alongside it and are loaded on
demand:

- `references/metrics.md` — what each metric (`loc`, `ccn`,
  `cognitive`, `churn`, `change_coupling`, `duplication`, `hotspot`,
  `lcom`) measures, the literature behind it, the thresholds, and
  the typical false positives.
- `references/architecture.md` — the vocabulary for refactor
  proposals: module depth (Ousterhout), layered / hexagonal
  architecture (Cockburn, Evans), DDD (Evans, Vernon), the leverage
  hierarchy of refactor patterns, the trap catalogue, plus the
  rules for _respecting the codebase_ the proposals must pass.
- `references/readability.md` — the *positive* criterion for
  proposals: the goal hierarchy (readability → maintainability →
  metric), readability principles (Boswell, Ousterhout, Beck,
  Knuth), and the 5-question judgement test.

`/heal-code-review` proposes only — it never edits source. The write
counterpart is `/heal-code-patch`.

## The write skill: `/heal-code-patch`

`/heal-code-patch` is the repair loop counterpart to `/heal-code-review`.
It drains `.heal/findings/latest.json` one finding at a time, in
Severity order, committing once per fix.

Pre-flight (refuses to start when these fail):

1. **Clean worktree.** A dirty worktree means the cache's
   `worktree_clean = false` and the recorded numbers don't reflect
   the on-disk source. The skill stops and asks you to commit or
   stash first.
2. **Cache exists.** If `latest.json` is missing, the skill runs
   `heal status --json` once to populate it.
3. **Calibration exists.** Without `calibration.toml`, every Finding
   is `Severity::Ok` — nothing actionable.

The loop drains **T0 (`must`) only** — T1 / Advisory are surfaced for
review but never auto-drained. End the session when T0 is empty rather
than silently extending into T1.

```
while there are findings in T0 of the cache:
    pick the next one (Severity 🔥 desc within T0)
    read the file(s); plan the smallest fix that addresses the metric
    apply the change
    run tests / type-check / linter (best effort)
    git add ...; git commit -m "<conventional message + Refs: F#<id>>"
    heal mark-fixed --finding-id <id> --commit-sha <sha>
    heal status --refresh --json   # re-scan; reconcile fixed.json ↔ regressed.jsonl
    if the finding regressed: leave it for now, continue with the next
    else: continue
```

Stop conditions: T0 empty, user interrupts (Ctrl+C / Stop), or the
skill hits a finding that needs human judgement (architectural
decision, business rule). In the last case, it surfaces the
trade-offs and asks before applying. When T0 is empty but T1 / Advisory
findings remain, the skill ends with a summary and recommends running
`/heal-code-review` for proposal-level discussion.

Per-metric, `/heal-code-patch` maps to established refactoring vocabulary
(Fowler, Tornhill):

| Metric                      | Common moves                                                                           |
| --------------------------- | -------------------------------------------------------------------------------------- |
| `ccn` / `cognitive`         | Extract Function, Replace Nested Conditional with Guard Clauses, Decompose Conditional |
| `duplication`               | Extract Function / Method, Pull Up Method, Form Template Method, Rule of Three         |
| `change_coupling`           | Surface the architectural seam — `/heal-code-patch` does not auto-fix coupling           |
| `change_coupling.symmetric` | Same — strong "responsibility mixing" signal needs a human call                        |
| `lcom`                      | Split the class along the cluster boundary — usually Extract Class                     |
| `hotspot`                   | Hotspot is a flag, not a problem; act on the underlying CCN/dup/coupling               |

Constraints (enforced by the skill):

- One finding = one commit. No squashing across findings.
- Conventional Commit subject + body + `Refs: F#<finding_id>` trailer.
- Never push, never amend, never `--no-verify`.
- Never extends the loop beyond the cache. New findings the user wants
  addressed go into a fresh `heal status` run.

## The helper skills: `/heal-cli` and `/heal-config`

Two non-loop skills round out the bundle, aimed at giving Claude
direct reference material rather than a multi-step procedure.

`/heal-cli` is a concise, complete reference for the `heal` CLI —
every subcommand, every `--json` shape, and the `.heal/` files each
command reads or writes. Claude loads it before shelling out to
`heal` from any other skill, so the CLI surface is treated as a
stable contract instead of being inferred from `--help` text.

`/heal-config` calibrates the project, surveys the codebase, asks the
user to pick a strictness level (Strict / Default / Lenient), and
writes or updates `.heal/config.toml` accordingly. Its
`references/config.md` is the complete schema for every key in
`config.toml` plus the per-strictness recipe table. Use it when
setting heal up for the first time, after a structural change to the
codebase (a new vendored tree, a layer rewrite), or when you want to
shift the quality bar without remembering every threshold.

`/heal-config` also owns calibration drift detection (which the CLI
no longer does). On request — or as part of any config update — it
reads `calibration.toml.meta.calibrated_at_sha` and `codebase_files`,
compares them to the current `.heal/findings/latest.json` and
`.heal/findings/fixed.json`, and recommends `heal calibrate --force`
when the calibration baseline has drifted (file count moved
significantly, the calibration is old relative to project velocity,
or every Critical has been drained for a sustained run). The check
is idempotent — running the skill repeatedly without intervening
changes produces the same recommendation.

## Updating the skills

After upgrading the `heal` binary:

```sh
heal skills update
```

**Drift-aware, no manifest needed**. Each installed `SKILL.md`
carries a `metadata:` block in its YAML frontmatter (`heal-version`,
`heal-source`). `update` derives drift directly from the on-disk
bytes: it strips the metadata block and compares the remainder
against the bundled raw bytes.

- Files whose canonical (metadata-stripped) content matches the
  bundled bytes are overwritten with the new bundled version.
- Files with hand edits (anything outside the metadata block) are
  left in place, with a warning.
- Pass `--force` to overwrite everything, including hand edits.

`heal skills status` reports which files have drifted, side-by-side.
The same on-disk + bundled byte comparison runs on every machine, so
re-installs from different teammates produce the same verdict — no
sidecar manifest to coordinate.

## Removing it

```sh
heal skills uninstall
```

Removes:

- Every bundled skill directory under `.claude/skills/heal-*` that
  the install pass owns. Sibling skills you authored survive.
- Any legacy `heal hook edit` / `heal hook stop` entries in
  `.claude/settings.json` (current heal does not register them; this
  step exists for upgrades from older versions).
- Any **legacy** install layout left over from older heal versions
  that distributed via a marketplace plugin: the old
  `.claude/plugins/heal/` tree, `.claude-plugin/marketplace.json`,
  and the `extraKnownMarketplaces["heal-local"]` /
  `enabledPlugins["heal@heal-local"]` entries in `settings.json`.

Project data under `.heal/` is otherwise left untouched.

If you are upgrading from a heal version that still distributed via a
plugin marketplace, the safe migration path is one
`heal skills uninstall` followed by `heal skills install`. (`install`
and `update` intentionally do not migrate the old layout.)

## Why it is bundled

A single distribution channel — `cargo install heal-cli` — provides
both the CLI and the matching skills. Lock-step versioning prevents
accidentally pairing mismatched skill and binary versions. The
trade-off is that the skill set is exactly as fresh as the `heal`
binary; to revise skill prompts independently, hand-edit
`.claude/skills/heal-*/`, with the understanding that
`heal skills update` will then mark those files as drifted.
