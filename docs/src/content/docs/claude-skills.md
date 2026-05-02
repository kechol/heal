---
title: Claude skills
description: How heal's bundled Claude Code skills connect heal's metrics to Claude sessions, with the /heal-code-review audit and /heal-code-patch repair loop, plus heal-cli and heal-config helper skills.
---

heal ships with a bundled set of Claude Code skills so the metrics it
collects flow into Claude sessions automatically. They are installed
once per repository with `heal skills install`. From that point on:

- Every Claude edit and turn-end is logged to `.heal/logs/` via inline
  `settings.json` hooks (no shell-script wrappers).
- A read-only skill `/heal-code-review` audits
  `.heal/checks/latest.json` and produces an architectural reading
  plus a prioritised refactor TODO list.
- A write skill `/heal-code-patch` drains the same cache one finding
  per commit, in Severity order, until the cache is empty or you
  stop the session.
- Two helper skills, `/heal-cli` and `/heal-config`, give Claude
  reference material for driving the CLI and tuning `config.toml`.

The pre-v0.2 SessionStart nudge has been retired. The post-commit
hook (run by `heal init`'s git installation) handles the same role
with simpler semantics — see
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

## How the hooks are wired

`heal skills install` also merges two entries into
`<project>/.claude/settings.json`:

```jsonc
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit|Write|MultiEdit",
        "hooks": [{ "type": "command", "command": "heal hook edit" }]
      }
    ],
    "Stop": [
      { "hooks": [{ "type": "command", "command": "heal hook stop" }] }
    ]
  }
}
```

Both call back into the same `heal hook` entrypoint:

| Hook event    | Behaviour                                                                        |
| ------------- | -------------------------------------------------------------------------------- |
| `PostToolUse` | Logs every Edit / Write / MultiEdit Claude makes, to `.heal/logs/` (event-only). |
| `Stop`        | Logs the end of every Claude turn.                                               |

Both are pure logging — they do not run any observer, so they add no
measurable latency to a Claude turn.

The merge is **additive**: existing user hook entries are preserved
(deduped by exact `command` match), and `heal skills uninstall`
removes only HEAL's own command lines. Other entries you wrote stay.

`heal hook` itself is robust against being invoked in a project
without `.heal/`: it silently no-ops, so a stray Claude session in an
un-opted-in repository never materialises HEAL state. Edit / Stop
swallow internal failures so the inline command never blocks Claude's
loop; `commit` (invoked from a git hook) preserves the original error
path.

The repair loop runs through the `heal-code-patch` skill (below), not
a SessionStart nudge.

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
It drains `.heal/checks/latest.json` one finding at a time, in
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
    heal status --refresh --json   # re-scan; reconcile fixed.jsonl ↔ regressed.jsonl
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

## Updating the skills

After upgrading the `heal` binary:

```sh
heal skills update
```

**Drift-aware**. heal records the fingerprint of every installed file
in `.heal/skills-install.json`. On update:

- Files matching the recorded bundled fingerprint are overwritten
  with the new bundled version.
- Files with a different fingerprint (hand-edited) are left in place,
  with a warning.
- Pass `--force` to overwrite everything, including hand edits.

`heal skills status` reports which files have drifted, side-by-side.

## Removing it

```sh
heal skills uninstall
```

Removes:

- Every skill directory under `.claude/skills/heal-*` that the
  manifest recorded.
- `.heal/skills-install.json`.
- HEAL's own command entries in `.claude/settings.json`. Other user
  hooks survive untouched; if no other entries remain the file is
  deleted.
- Any **legacy** install layout left over from older heal versions
  that distributed via a marketplace plugin: the old
  `.claude/plugins/heal/` tree, `.claude-plugin/marketplace.json`,
  and the `extraKnownMarketplaces["heal-local"]` /
  `enabledPlugins["heal@heal-local"]` entries in `settings.json`.

Project data under `.heal/` is otherwise left untouched.

If you are upgrading from a heal version that still distributed via a
plugin marketplace, the safe migration path is one
`heal skills uninstall` followed by `heal skills install`. (`install`
and `update` intentionally do not migrate the old layout — running
the new binary alongside the old one will fire hooks twice until you
uninstall.)

## Why it is bundled

A single distribution channel — `cargo install heal-cli` — provides
both the CLI and the matching skills. Lock-step versioning prevents
accidentally pairing mismatched skill and binary versions. The
trade-off is that the skill set is exactly as fresh as the `heal`
binary; to revise skill prompts independently, hand-edit
`.claude/skills/heal-*/`, with the understanding that
`heal skills update` will then mark those files as drifted.
