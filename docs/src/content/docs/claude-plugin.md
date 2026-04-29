---
title: Claude plugin
description: How the bundled Claude Code plugin connects heal's metrics to Claude sessions.
---

heal ships with a Claude Code plugin so the metrics it collects can
flow into Claude sessions automatically. The plugin is installed once
per repository with `heal skills install`. From that point on:

- Every Claude edit and turn-end is logged.
- When a metric crosses a threshold, the next Claude session opens
  with a notice at the top.
- Five `check-*` skills become available for asking Claude about a
  specific metric on demand.

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
│   ├── claude-stop.sh
│   └── claude-session-start.sh
└── skills/
    ├── check-overview/SKILL.md
    ├── check-hotspots/SKILL.md
    ├── check-complexity/SKILL.md
    ├── check-duplication/SKILL.md
    └── check-coupling/SKILL.md
```

The plugin tree is embedded in the `heal` binary at compile time, so
the version installed always matches the binary. After upgrading
`heal`, run `heal skills update` to refresh.

## What the hooks do

Three hooks ship in v0.1. All three call back into the same `heal
hook` entrypoint.

| Hook event     | Behaviour                                                                  |
| -------------- | -------------------------------------------------------------------------- |
| `PostToolUse`  | Logs every Edit / Write / MultiEdit Claude makes, to `.heal/logs/`.        |
| `Stop`         | Logs the end of every Claude turn.                                         |
| `SessionStart` | Reads the latest snapshot delta and emits a notice if a threshold crossed. |

The first two are pure logging — they do not run any observer, so
they add no measurable latency to a Claude turn.

The most consequential of the three is `SessionStart`. When a Claude
session opens it:

1. Loads the latest `MetricsSnapshot` from `.heal/snapshots/`.
2. Compares it to the previous snapshot.
3. Evaluates five v0.1 rules (new top hotspot, new top CCN function,
   new top Cognitive function, CCN spike, duplication growth).
4. For any rule that fires and whose cool-down (default 24 hours)
   has expired, prints a markdown notice that Claude sees at the top
   of the session.
5. Updates `.heal/runtime/state.json` so the same rule does not
   re-fire until the cool-down expires.

Cool-downs are per-rule, so a different breach can fire on the next
session without waiting.

## The five `check-*` skills

Read-only Claude skills that wrap a specific `heal status --metric
<X>` call.

| Skill               | Function                                                                         |
| ------------------- | -------------------------------------------------------------------------------- |
| `check-overview`    | Synthesises every enabled metric into a single situation report.                 |
| `check-hotspots`    | Drills into the hotspot ranking and explains why each top file scored.           |
| `check-complexity`  | Walks through the worst CCN / Cognitive functions and suggests refactor targets. |
| `check-duplication` | Reviews duplicate blocks and suggests where helpers might be extracted.          |
| `check-coupling`    | Reviews co-change pairs and suggests where an abstraction may be missing.        |

Two ways to invoke a skill:

- From the terminal: `heal check hotspots` — runs Claude in headless
  mode (`claude -p`).
- Inside an interactive Claude session: ask Claude to use the
  `check-hotspots` skill.

All five skills are read-only — they may run `heal status` but cannot
modify source files. The `run-*` repair skills land in v0.2.

## Updating the plugin

After upgrading the `heal` binary:

```sh
heal skills update
```

This is **drift-aware**. heal records the fingerprint of every file
it installs in `.claude/plugins/heal/.heal-install.json`. On update:

- Files matching the recorded bundled fingerprint are overwritten
  with the new bundled version.
- Files with a different fingerprint (those that have been
  hand-edited) are left in place, with a warning.
- Pass `--force` to overwrite everything, including hand edits.

`heal skills status` reports which files have drifted, with a
side-by-side bundled / installed comparison.

## Removing it

```sh
heal skills uninstall
```

Removes `.claude/plugins/heal/` and nothing else. Project data under
`.heal/` is left untouched.

## Why it is bundled

A single distribution channel — `cargo install heal-cli` — provides
both the CLI and the matching plugin. Lock-step versioning prevents
accidentally pairing a v0.2 plugin with a v0.1 binary or vice versa.
The trade-off is that the plugin is exactly as fresh as the `heal`
binary; to revise skill prompts independently, hand-edit
`.claude/plugins/heal/`, with the understanding that `heal skills
update` will then mark those files as drifted.
