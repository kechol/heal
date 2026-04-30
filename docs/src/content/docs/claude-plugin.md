---
title: Claude plugin
description: How the bundled Claude Code plugin connects heal's metrics to Claude sessions, including the /heal-fix repair loop.
---

heal ships with a Claude Code plugin so the metrics it collects flow
into Claude sessions automatically. The plugin is installed once per
repository with `heal skills install`. From that point on:

- Every Claude edit and turn-end is logged to `.heal/logs/`.
- Five read-only `check-*` skills are available for asking Claude
  about a specific metric on demand.
- The write skill `/heal-fix` drains `.heal/checks/latest.json` one
  finding per commit, in Severity order, until the cache is empty or
  you stop the session.

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
    ├── check-overview/SKILL.md
    ├── check-hotspots/SKILL.md
    ├── check-complexity/SKILL.md
    ├── check-duplication/SKILL.md
    ├── check-coupling/SKILL.md
    └── heal-fix/SKILL.md
```

The plugin tree is embedded in the `heal` binary at compile time, so
the version installed always matches the binary. After upgrading
`heal`, run `heal skills update` to refresh.

## What the hooks do

Two hooks ship with the plugin. Both call back into the same
`heal hook` entrypoint.

| Hook event    | Behaviour                                                                       |
| ------------- | ------------------------------------------------------------------------------- |
| `PostToolUse` | Logs every Edit / Write / MultiEdit Claude makes, to `.heal/logs/` (event-only). |
| `Stop`        | Logs the end of every Claude turn.                                              |

Both are pure logging — they do not run any observer, so they add no
measurable latency to a Claude turn.

The repair loop runs through the `heal-fix` skill (below), not a
SessionStart nudge.

## The five `check-*` skills

Read-only skills that wrap a `heal status --metric <X>` call and
explain the resulting numbers in the project's `response_language`.

| Skill               | Function                                                                         |
| ------------------- | -------------------------------------------------------------------------------- |
| `check-overview`    | Synthesises every enabled metric into a single situation report.                 |
| `check-hotspots`    | Drills into the hotspot ranking and explains why each top file scored.           |
| `check-complexity`  | Walks through the worst CCN / Cognitive functions and suggests refactor targets. |
| `check-duplication` | Reviews duplicate blocks and suggests where helpers might be extracted.          |
| `check-coupling`    | Reviews co-change pairs and suggests where an abstraction may be missing.        |

Two ways to invoke a skill:

- From the terminal: `heal check overview` — the legacy positional
  alias maps to `--metric` flags and prints a deprecation warning.
- Inside an interactive Claude session: ask Claude to use the
  `check-hotspots` skill (or any of the five) by name.

All five are read-only — they may run `heal status` but cannot modify
source files.

## The write skill: `/heal-fix`

`/heal-fix` is the repair loop counterpart to the `check-*` skills.
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
    heal cache mark-fixed --finding-id <id> --commit-sha <sha>
    heal check --json   # re-scan; reconcile fixed.jsonl ↔ regressed.jsonl
    if the finding regressed: leave it for now, continue with the next
    else: continue
```

Stop conditions: cache empty, user interrupts (Ctrl+C / Stop), or the
skill hits a finding that needs human judgement (architectural
decision, business rule). In the last case, it surfaces the
trade-offs and asks before applying.

Per-metric, `/heal-fix` maps to established refactoring vocabulary
(Fowler, Tornhill):

| Metric              | Common moves                                                                |
| ------------------- | --------------------------------------------------------------------------- |
| `ccn` / `cognitive` | Extract Function, Replace Nested Conditional with Guard Clauses, Decompose Conditional |
| `duplication`       | Extract Function / Method, Pull Up Method, Form Template Method, Rule of Three |
| `change_coupling`   | Surface the architectural seam — `/heal-fix` does not auto-fix coupling     |
| `change_coupling.symmetric` | Same — strong "responsibility mixing" signal needs a human call         |
| `lcom`              | Split the class along the cluster boundary — usually Extract Class          |
| `hotspot`           | Hotspot is a flag, not a problem; act on the underlying CCN/dup/coupling    |

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
