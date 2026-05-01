---
title: Claude plugin
description: How the bundled Claude Code plugin connects heal's metrics to Claude sessions, with the /heal-code-review audit and /heal-code-patch repair loop.
---

heal ships with a Claude Code plugin so the metrics it collects flow
into Claude sessions automatically. The plugin is installed once per
repository with `heal skills install`. From that point on:

- Every Claude edit and turn-end is logged to `.heal/logs/`.
- A read-only skill `/heal-code-review` audits
  `.heal/checks/latest.json` and produces an architectural reading
  plus a prioritised refactor TODO list.
- A write skill `/heal-code-patch` drains the same cache one finding
  per commit, in Severity order, until the cache is empty or you
  stop the session.

The pre-v0.2 SessionStart nudge has been retired. The post-commit
hook (run by `heal init`'s git installation) handles the same role
with simpler semantics — see
[Architecture › The big picture](/heal/architecture/#the-big-picture).

## Installing it

```sh
heal skills install
```

This extracts the plugin tree to `.claude/plugins/heal/`:

```
.claude/plugins/heal/
├── plugin.json
├── hooks/
│   ├── claude-post-tool-use.sh
│   └── claude-stop.sh
└── skills/
    ├── heal-code-review/
    │   ├── SKILL.md
    │   └── references/
    │       ├── metrics.md
    │       └── architecture.md
    └── heal-code-patch/
        └── SKILL.md
```

The plugin tree is embedded in the `heal` binary at compile time, so
the version installed always matches the binary. After upgrading
`heal`, run `heal skills update` to refresh.

## What the hooks do

Two hooks ship with the plugin. Both call back into the same
`heal hook` entrypoint.

| Hook event    | Behaviour                                                                        |
| ------------- | -------------------------------------------------------------------------------- |
| `PostToolUse` | Logs every Edit / Write / MultiEdit Claude makes, to `.heal/logs/` (event-only). |
| `Stop`        | Logs the end of every Claude turn.                                               |

Both are pure logging — they do not run any observer, so they add no
measurable latency to a Claude turn.

The repair loop runs through the `heal-code-patch` skill (below), not
a SessionStart nudge.

## The audit skill: `/heal-code-review`

Read-only. Ingests `heal check --all --json`, deep-reads the flagged
code, and returns two artefacts:

1. An **architectural reading** of the codebase — what the findings
   say _as a system_, not as a list (the dominant axis: complexity,
   duplication, coupling, or hub).
2. A **prioritised TODO list** — concrete refactors keyed to specific
   files / functions, each tagged with the established refactor
   pattern and the expected metric movement.

The skill is language-agnostic and tailors proposals to the
codebase's apparent style instead of pushing a one-size-fits-all
template. Two reference files ship alongside it and are loaded on
demand:

- `references/metrics.md` — what each metric (`loc`, `ccn`,
  `cognitive`, `churn`, `change_coupling`, `duplication`, `hotspot`,
  `lcom`) measures, the literature behind it, the thresholds, and
  the typical false positives.
- `references/architecture.md` — the vocabulary for refactor
  proposals: module depth (Ousterhout), layered / hexagonal
  architecture (Cockburn, Evans), DDD (Evans, Vernon), plus the
  rules for _respecting the codebase_ the proposals must pass.

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
   `heal check --json` once to populate it.
3. **Calibration exists.** Without `calibration.toml`, every Finding
   is `Severity::Ok` — nothing actionable.

The loop:

```
while there are non-Ok findings in the cache:
    pick the next one (Severity order: Critical🔥 → Critical → High🔥 → High → Medium)
    read the file(s); plan the smallest fix that addresses the metric
    apply the change
    run tests / type-check / linter (best effort)
    git add ...; git commit -m "<conventional message + Refs: F#<id>>"
    heal fix mark --finding-id <id> --commit-sha <sha>
    heal check --refresh --json   # re-scan; reconcile fixed.jsonl ↔ regressed.jsonl
    if the finding regressed: leave it for now, continue with the next
    else: continue
```

Stop conditions: cache empty, user interrupts (Ctrl+C / Stop), or the
skill hits a finding that needs human judgement (architectural
decision, business rule). In the last case, it surfaces the
trade-offs and asks before applying.

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
  addressed go into a fresh `heal check` run.

## Updating the plugin

After upgrading the `heal` binary:

```sh
heal skills update
```

**Drift-aware**. heal records the fingerprint of every installed file
in `.claude/plugins/heal/.heal-install.json`. On update:

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

Removes `.claude/plugins/heal/` and nothing else. Project data under
`.heal/` is left untouched.

## Why it is bundled

A single distribution channel — `cargo install heal-cli` — provides
both the CLI and the matching plugin. Lock-step versioning prevents
accidentally pairing mismatched plugin and binary versions. The
trade-off is that the plugin is exactly as fresh as the `heal`
binary; to revise skill prompts independently, hand-edit
`.claude/plugins/heal/`, with the understanding that
`heal skills update` will then mark those files as drifted.
