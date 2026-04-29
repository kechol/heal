---
title: Architecture
description: Where heal stores data, what gets written and when, and how the pieces fit together.
---

This page explains what files heal creates, when they are written, and
what each one contains. It is useful when debugging a missing nudge,
scripting against the JSON output, or simply wanting to understand what
heal is doing in the background.

## The big picture

```
git commit
    │
    ▼
.git/hooks/post-commit  ──►  heal hook commit
                                  │
                                  ├──►  observers (LOC, complexity, churn, …)
                                  │
                                  ├──►  .heal/snapshots/YYYY-MM.jsonl
                                  │
                                  └──►  .heal/logs/YYYY-MM.jsonl

claude session opens
    │
    ▼
SessionStart hook  ──►  heal hook session-start
                             │
                             ├──►  read latest snapshot + delta
                             │
                             ├──►  read .heal/state.json (cool-downs)
                             │
                             └──►  print markdown nudge to stdout (Claude sees it)
```

`heal` is a single binary; both paths go through it. There is no
daemon, no scheduler, no background process.

## On-disk layout

After `heal init`:

```
<your-repo>/
├── .heal/
│   ├── config.toml                # you edit this; heal init writes the default
│   ├── snapshots/
│   │   └── 2026-04.jsonl          # full metrics snapshot per commit
│   ├── logs/
│   │   └── 2026-04.jsonl          # lightweight event timeline
│   └── state.json                 # cool-down timestamps for the nudge rules
│
├── .git/hooks/post-commit         # one-line shim: calls `heal hook commit`
│
└── .claude/plugins/heal/          # Claude plugin (after `heal skills install`)
```

## What gets written and when

| File                            | Written by                 | When                                     |
| ------------------------------- | -------------------------- | ---------------------------------------- |
| `.heal/config.toml`             | `heal init`                | Once at setup; you can edit it freely.   |
| `.heal/snapshots/YYYY-MM.jsonl` | post-commit hook           | On every `git commit`.                   |
| `.heal/logs/YYYY-MM.jsonl`      | post-commit + Claude hooks | On every commit and Claude event.        |
| `.heal/state.json`              | SessionStart hook          | Updated each time a rule fires.          |
| `.claude/plugins/heal/`         | `heal skills install`      | Once; updated with `heal skills update`. |

## The event log

Both `snapshots/` and `logs/` share the same on-disk format:

- **One file per month**: `YYYY-MM.jsonl` (UTC).
- **Append-only**: every record is one JSON object on one line.

Every record has the same outer shape:

```json
{
  "timestamp": "2026-04-29T05:14:22Z",
  "event": "commit",
  "data": {
    /* … shape depends on event … */
  }
}
```

The `event` field tells you what kind of payload `data` is.

### `snapshots/` — metric payloads

Written on every commit. Contains the full output from every enabled
observer. This is what `heal status` reads.

```json
{
  "version": 1,
  "git_sha": "a0a6d1a…",
  "loc": {
    /* LocReport */
  },
  "complexity": {
    /* or null if disabled */
  },
  "churn": {
    /* … */
  },
  "change_coupling": null,
  "duplication": {
    /* … */
  },
  "hotspot": {
    /* … */
  },
  "delta": {
    /* SnapshotDelta, or null on the first snapshot */
  }
}
```

`delta` summarises what changed since the previous snapshot — new
entries in worst-N lists, changes in `max_ccn`, hotspot ranking
shifts. The SessionStart nudge consumes it to decide which rules fire.

### `logs/` — event timeline

Lightweight records written on every commit and every Claude hook
event. This is what `heal logs` reads.

| Event type      | Written when                                 |
| --------------- | -------------------------------------------- |
| `init`          | `heal init` ran                              |
| `commit`        | A `git commit` landed (commit metadata only) |
| `edit`          | Claude edited a file (PostToolUse hook)      |
| `stop`          | A Claude turn ended (Stop hook)              |
| `session-start` | A Claude session opened (SessionStart hook)  |

`commit` events in `logs/` carry lightweight metadata (sha, author,
message summary, files changed) — not the full metrics payload. That
split keeps timeline queries fast regardless of how many metrics are
enabled.

You can inspect either stream directly with standard Unix tools:

```sh
# last 5 commit events
heal logs --filter commit --limit 5

# raw JSON for scripting
heal logs --json | jq '.data.git_sha'
```

## State

`.heal/state.json` tracks cool-down timestamps so the same
nudge does not appear in every session:

```json
{
  "last_fired": {
    "complexity.spike": "2026-04-28T03:14:22Z",
    "hotspot.new_top": "2026-04-25T11:02:08Z"
  }
}
```

When a rule fires, heal records the timestamp here. The next
SessionStart suppresses that rule until `cooldown_hours` have passed.
Writes are atomic (write-to-temp + rename) so an interrupted process
cannot leave the file half-written.
