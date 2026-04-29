---
title: CLI
description: Every heal subcommand you will actually use, with examples.
---

`heal` is a single binary. Every interaction goes through one of the
subcommands below. Run `heal --help` or `heal <subcommand> --help`
any time for the full argument list — this page covers the parts you
will use day to day.

## Quick reference

| Command       | What it does                                                                                  |
| ------------- | --------------------------------------------------------------------------------------------- |
| `heal init`   | Set up `.heal/` and the post-commit hook in the current repo. Run once.                       |
| `heal status` | Show the latest metrics and what changed since last commit.                                   |
| `heal logs`   | Stream the event log (commits, Claude edits, session starts).                                 |
| `heal check`  | Ask Claude Code to read the metrics and explain them.                                         |
| `heal skills` | Install / update / remove the bundled Claude plugin.                                          |
| `heal hook`   | Internal entrypoint called by git and the Claude plugin. You usually do not run this by hand. |

## `heal init`

Bootstraps HEAL inside a git repository:

```sh
heal init
```

This creates `.heal/` (with `config.toml`, `snapshots/`, `logs/`),
installs `.git/hooks/post-commit`, and captures one initial snapshot.
It is **safe to re-run** — it will leave your config alone unless
you pass `--force`. The post-commit hook is marked with a comment so
re-installs never duplicate it.

If you already have a `post-commit` hook of your own, `heal init`
notices and refuses to overwrite. Pass `--force` if you actually want
to.

## `heal status`

Your everyday command:

```sh
heal status
heal status --json
heal status --metric complexity
```

It prints a summary of every enabled metric — primary language,
worst-N complex functions, top hotspots, and so on — plus a "delta"
block showing what moved since the previous commit's snapshot.

`--metric <name>` scopes the output to a single metric. Valid names:
`loc`, `complexity`, `churn`, `change-coupling`, `duplication`,
`hotspot`. `--json` is the same data as machine-parseable JSON for
piping into `jq`.

If `.heal/snapshots/` is empty (you ran `heal init` but have not
committed yet), `heal status` will tell you so.

## `heal logs`

Reads the event log under `.heal/logs/`:

```sh
heal logs
heal logs --filter commit --limit 10
heal logs --since 2026-04-01T00:00:00Z
heal logs --json
```

Every record is one line of JSON. There are five event types:

- `init` — written once by `heal init`
- `commit` — written by the git post-commit hook
- `edit` — written when Claude edits a file (via the plugin)
- `stop` — written when a Claude turn ends
- `session-start` — written when a Claude session opens

`heal status` reads `snapshots/` (the heavy metric payloads); `heal
logs` reads `logs/` (the lightweight event timeline). They are
complementary.

## `heal check`

Hands the latest metrics to Claude Code with a read-only prompt:

```sh
heal check                    # default: an overview of everything
heal check hotspots           # drill into hotspot ranking
heal check complexity         # CCN + Cognitive walkthrough
heal check duplication
heal check coupling
```

Each variant calls `claude -p` (Claude in headless mode) with a small
"check-\*" skill that knows which metric to focus on. The skill
files are part of the bundled plugin — see [Claude plugin](/heal/claude-plugin/).

You can pass arguments straight through to `claude`:

```sh
heal check overview -- --model sonnet --effort medium
```

Anything after the `--` goes verbatim to `claude`, so use it for
`--model`, `--effort`, `--no-cache`, etc.

`heal check` does not modify your code. It is a "explain to me what
HEAL just measured" command.

## `heal skills`

Manages the bundled Claude plugin under `.claude/plugins/heal/`:

```sh
heal skills install     # extract the plugin (run once per repo)
heal skills update      # refresh after upgrading the heal binary
heal skills status      # what is installed vs. what is bundled
heal skills uninstall   # remove .claude/plugins/heal/
```

The plugin tree is **embedded in the heal binary** at compile time,
so a fresh `heal skills install` always matches the version of `heal`
you are running. `update` is drift-aware: it leaves files you have
hand-edited alone (use `--force` to overwrite anyway).

## `heal hook`

You almost never call this yourself — `heal init` and `heal skills
install` wire it up automatically. It exists as a single entrypoint
the various hooks share:

```sh
heal hook commit          # post-commit: run observers, write a snapshot
heal hook edit            # Claude PostToolUse: log only, no scan
heal hook stop            # Claude Stop: log only
heal hook session-start   # Claude SessionStart: emit the threshold nudge
```

The only one you might call by hand is `heal hook session-start`
when debugging a missing nudge — pipe an empty JSON object in and see
what the snapshot delta would have produced.

## Tips

- **Run `heal status` after every interesting commit.** It is fast
  and it gives you a reality check before opening a Claude session.
- **`heal check` is the friendly version of `heal status`.** When the
  numbers are confusing, ask Claude. The check skills are just
  prompts that pass `heal status --metric <X>` through.
- **Keep the post-commit hook.** Removing it breaks the snapshot
  chain — `heal status` will keep showing the same delta forever.
