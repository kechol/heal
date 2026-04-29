---
title: Claude plugin
description: How the bundled Claude Code plugin connects HEAL's metrics to your Claude sessions.
---

HEAL ships with a Claude Code plugin so the metrics it collects can
flow into your Claude sessions automatically. You install it once
per repo with `heal skills install`, and from then on:

- Every Claude edit and turn-end is logged.
- When a metric crosses a threshold, the next Claude session opens
  with a friendly nudge at the top.
- You get five `check-*` skills you can invoke explicitly to ask
  Claude about a specific metric.

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

The plugin is **bundled into the `heal` binary** at compile time, so
the version you install always matches the binary that produced it.
After upgrading `heal`, run `heal skills update` to refresh.

## What the hooks do

Three hooks ship in v0.1. All three call back into the same `heal
hook` entrypoint.

| Hook event     | What it does                                                            |
| -------------- | ----------------------------------------------------------------------- |
| `PostToolUse`  | Logs every Edit / Write / MultiEdit Claude makes, to `.heal/logs/`.     |
| `Stop`         | Logs the end of every Claude turn.                                      |
| `SessionStart` | Reads the latest snapshot delta and emits a nudge if a threshold trips. |

The first two are pure logging — they do not run any observer, so
they add zero latency to a Claude turn.

The `SessionStart` hook is the interesting one. When you open a
Claude session, it:

1. Loads the latest `MetricsSnapshot` from `.heal/snapshots/`.
2. Compares it to the previous snapshot.
3. Evaluates five v0.1 rules (new top hotspot, new top CCN function,
   new top Cognitive function, CCN spike, duplication growth).
4. For any rule that fires _and_ whose cool-down (default 24 hours)
   has expired, prints a markdown nudge that Claude sees at the top
   of the session.
5. Updates `.heal/runtime/state.json` so the same rule will not
   nudge again until the cool-down passes.

Cool-downs are per-rule, so you can fire on a _different_ breach
the next session without waiting.

## The five `check-*` skills

These are read-only Claude skills that wrap a specific
`heal status --metric <X>` call.

| Skill               | Asks Claude to…                                                                   |
| ------------------- | --------------------------------------------------------------------------------- |
| `check-overview`    | Synthesise every enabled metric into a single situation report.                   |
| `check-hotspots`    | Drill into the hotspot ranking and explain why each top file scored.              |
| `check-complexity`  | Walk through the worst CCN / Cognitive functions and suggest refactor directions. |
| `check-duplication` | Look at duplicate blocks and suggest whether to extract helpers.                  |
| `check-coupling`    | Look at co-change pairs and ask whether there is a missing abstraction.           |

Two ways to invoke a skill:

- From your terminal: `heal check hotspots` — runs Claude in headless
  mode (`claude -p`).
- Inside an interactive Claude session: ask Claude to use the
  `check-hotspots` skill.

All five skills are read-only — they can run `heal status` but not
modify your code. The `run-*` repair skills land in v0.2.

## Updating the plugin

After upgrading the `heal` binary:

```sh
heal skills update
```

This is **drift-aware**. HEAL records the fingerprint of every file
it installs in `.claude/plugins/heal/.heal-install.json`. On
update:

- Files that match the recorded bundled fingerprint are overwritten
  with the new bundled version.
- Files whose fingerprint differs (i.e. you hand-edited them) are
  left alone, with a warning.
- Pass `--force` to overwrite everything, including hand edits.

`heal skills status` shows which files have drifted and the side-by-
side bundled / installed version.

## Removing it

```sh
heal skills uninstall
```

Removes `.claude/plugins/heal/` and nothing else. Your project's
`.heal/` data is untouched.

## Why it is bundled

A single distribution channel — `cargo install heal-cli` — gives you
both the CLI and the matching plugin. Keeping them lock-step means
you never accidentally run a v0.2 plugin against a v0.1 binary or
vice versa. The trade-off is that the plugin is exactly as fresh as
your `heal` binary; to bump skill prompts independently you would
hand-edit `.claude/plugins/heal/`, accepting that `heal skills
update` will then mark those files as drifted.
