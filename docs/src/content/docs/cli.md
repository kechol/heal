---
title: CLI
description: The heal subcommand surface, with examples for everyday operations.
---

`heal` is a single binary. Every interaction goes through one of the
subcommands below. Run `heal --help` or `heal <subcommand> --help`
for the full argument list. This page covers the commands used in
day-to-day operations.

## Quick reference

| Command       | Purpose                                                                                |
| ------------- | -------------------------------------------------------------------------------------- |
| `heal init`   | Set up `.heal/` and the post-commit hook in the current repository. Run once.          |
| `heal status` | Show the latest metrics and the delta since the previous commit.                       |
| `heal logs`   | Stream the event log (commits, Claude edits, session starts).                          |
| `heal check`  | Invoke Claude Code to read the metrics and explain them.                               |
| `heal skills` | Install, update, or remove the bundled Claude plugin.                                  |
| `heal hook`   | Internal entrypoint called by git and the Claude plugin. Not normally invoked by hand. |

## `heal init`

Bootstraps HEAL inside a git repository:

```sh
heal init
```

This command creates `.heal/` (with `config.toml`, `snapshots/`,
`logs/`), installs `.git/hooks/post-commit`, and captures one initial
snapshot. It is safe to re-run: the config is left in place unless
`--force` is passed, and the post-commit hook is marked with a comment
so re-installation never duplicates the line.

If a `post-commit` hook already exists, `heal init` does not overwrite
it. Pass `--force` to replace the existing hook.

## `heal status`

The primary status command:

```sh
heal status
heal status --json
heal status --metric complexity
```

Prints a summary of every enabled metric — primary language, worst-N
complex functions, top hotspots — together with a delta block showing
movement since the previous commit.

`--metric <name>` scopes the output to a single metric. Valid names:
`loc`, `complexity`, `churn`, `change-coupling`, `duplication`,
`hotspot`. `--json` produces the same data as machine-readable JSON,
suitable for piping into `jq`.

If `.heal/snapshots/` is empty (for example, immediately after
`heal init` and before the first commit), the command reports that
no snapshots are available.

## `heal logs`

Reads the event log under `.heal/logs/`:

```sh
heal logs
heal logs --filter commit --limit 10
heal logs --since 2026-04-01T00:00:00Z
heal logs --json
```

Each record is a single line of JSON. Five event types are produced:

- `init` — written once by `heal init`
- `commit` — written by the git post-commit hook
- `edit` — written when Claude edits a file (via the plugin)
- `stop` — written when a Claude turn ends
- `session-start` — written when a Claude session opens

`heal status` reads `snapshots/` (the heavy metric payloads); `heal
logs` reads `logs/` (the lightweight event timeline). The two are
complementary.

## `heal check`

Passes the latest metrics to Claude Code with a read-only prompt:

```sh
heal check                    # default: an overview of every metric
heal check hotspots           # hotspot ranking
heal check complexity         # CCN and Cognitive walkthrough
heal check duplication
heal check coupling
```

Each variant invokes `claude -p` (Claude in headless mode) with a
small `check-*` skill that scopes the prompt to the relevant metric.
The skill files are part of the bundled plugin — see
[Claude plugin](/heal/claude-plugin/).

Arguments after `--` are forwarded verbatim to `claude`:

```sh
heal check overview -- --model sonnet --effort medium
```

This is useful for `--model`, `--effort`, `--no-cache`, and similar
flags.

`heal check` is the explanatory counterpart to `heal status`; it does
not modify source files.

## `heal skills`

Manages the bundled Claude plugin under `.claude/plugins/heal/`:

```sh
heal skills install     # extract the plugin (run once per repository)
heal skills update      # refresh after upgrading the heal binary
heal skills status      # compare installed vs. bundled
heal skills uninstall   # remove .claude/plugins/heal/
```

The plugin tree is embedded in the `heal` binary at compile time, so
`heal skills install` always extracts the version matching the binary
in use. `update` is drift-aware: files that have been hand-edited are
left in place (use `--force` to overwrite anyway).

## `heal hook`

This command is invoked automatically by `heal init` (post-commit
hook) and `heal skills install` (Claude hooks). It exists as a single
entrypoint shared by both:

```sh
heal hook commit          # post-commit: run observers, write a snapshot
heal hook edit            # Claude PostToolUse: log only, no scan
heal hook stop            # Claude Stop: log only
heal hook session-start   # Claude SessionStart: emit threshold nudge
```

Manual invocation is occasionally useful when debugging — for
example, running `heal hook session-start` with an empty JSON
payload reveals which rules would fire from the current snapshot
delta.

## Tips

- **Run `heal status` after meaningful commits.** It is fast and
  serves as a sanity check before opening a Claude session.
- **`heal check` is the prose form of `heal status`.** Use it when
  the numbers need interpretation; the check skills wrap
  `heal status --metric <X>` with a focused prompt.
- **Preserve the post-commit hook.** Removing it stops new snapshots
  from being recorded, and `heal status` will continue showing the
  previous delta.
