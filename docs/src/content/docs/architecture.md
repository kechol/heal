---
title: Architecture
description: Where heal stores data, what gets written and when, and how the pieces fit together.
---

This page explains what files heal creates, when they are written, and
what each one contains. It is useful when debugging a missing nudge,
scripting against the JSON output, or simply wanting to understand
what heal is doing in the background.

## The big picture

```
git commit
    │
    ▼
.git/hooks/post-commit  ──►  heal hook commit
                                  │
                                  ├──►  observers (LOC, complexity, churn, …, lcom)
                                  │       (one run_all pass; reports reused below)
                                  │
                                  ├──►  .heal/snapshots/YYYY-MM.jsonl
                                  │       (MetricsSnapshot incl. severity_counts)
                                  │
                                  ├──►  .heal/logs/YYYY-MM.jsonl
                                  │       (lightweight CommitInfo)
                                  │
                                  └──►  stdout: Severity nudge
                                         (Critical / High findings only;
                                          recalibrate hint when a trigger fires)

user: heal check (or `claude /heal-fix`)
    │
    ▼
heal check  ──►  classify Findings via calibration.toml
                       │
                       ├──►  .heal/checks/YYYY-MM.jsonl + latest.json
                       │       (CheckRecord — the TODO list)
                       │
                       ├──►  reconcile fixed.jsonl ↔ regressed.jsonl
                       │
                       └──►  render Severity-grouped view to stdout
```

`heal` is a single binary; both paths go through it. There is no
daemon, no scheduler, no background process. The post-commit hook
runs every observer **once** and threads the result to both the
snapshot writer and the nudge — observers are not run twice per
commit.

## On-disk layout

After `heal init`:

```
<your-repo>/
├── .heal/
│   ├── config.toml                # you edit this
│   ├── calibration.toml           # auto — heal init / heal calibrate
│   ├── snapshots/
│   │   └── 2026-04.jsonl          # MetricsSnapshot + CalibrationEvent stream
│   ├── logs/
│   │   └── 2026-04.jsonl          # lightweight commit / edit / stop events
│   └── checks/
│       ├── 2026-04.jsonl          # append-only CheckRecord stream
│       ├── latest.json            # atomic mirror of the most recent record
│       ├── fixed.jsonl            # `/heal-fix` claimed a commit fixes a finding
│       └── regressed.jsonl        # a fix re-detected — surfaced as a warning
│
├── .git/hooks/post-commit         # one-line shim: calls `heal hook commit`
│
└── .claude/plugins/heal/          # Claude plugin (after `heal skills install`)
```

## What gets written and when

| File / dir                       | Written by                                  | When                                          |
| -------------------------------- | ------------------------------------------- | --------------------------------------------- |
| `.heal/config.toml`              | `heal init`                                 | Once at setup; you can edit it freely.        |
| `.heal/calibration.toml`         | `heal init` / `heal calibrate`              | At setup, then on explicit recalibration.     |
| `.heal/snapshots/YYYY-MM.jsonl`  | post-commit hook + `heal calibrate`         | On every `git commit` and recalibration.      |
| `.heal/logs/YYYY-MM.jsonl`       | post-commit + Claude PostToolUse / Stop     | On every commit and Claude tool event.        |
| `.heal/checks/YYYY-MM.jsonl`     | `heal check`                                | Each fresh `heal check` (cache-miss path).    |
| `.heal/checks/latest.json`       | `heal check`                                | Atomic mirror; refreshed on every fresh run.  |
| `.heal/checks/fixed.jsonl`       | `heal cache mark-fixed` (called by `/heal-fix`) | Each commit `/heal-fix` lands.            |
| `.heal/checks/regressed.jsonl`   | `heal check` (reconcile pass)               | When a fixed finding is re-detected.          |
| `.claude/plugins/heal/`          | `heal skills install`                       | Once; updated with `heal skills update`.      |

The pre-v0.2 `.heal/state.json` was retired along with the
SessionStart nudge — `EventLog::iter_segments` over `snapshots/` is
now the single way to query historical state.

## The event log

`snapshots/`, `logs/`, and `checks/` share the same on-disk format:

- **One file per month**: `YYYY-MM.jsonl` (UTC).
- **Append-only**: every record is one JSON object on one line.
- **Transparent gzip**: readers handle `.gz` files alongside plain
  text. Compaction (gzip past months, archive past 12) lands in
  v0.2+.

Every record has the same outer shape:

```json
{
  "timestamp": "2026-04-29T05:14:22Z",
  "event": "commit",
  "data": { /* … shape depends on event … */ }
}
```

The `event` field tells you what kind of payload `data` is.

### `snapshots/` — metric payloads

Written on every commit (`event = "commit"`) and every recalibration
(`event = "calibrate"`). The two co-exist; readers filter by `event`
before decoding.

```json
{
  "version": 1,
  "git_sha": "a0a6d1a…",
  "loc": { /* LocReport */ },
  "complexity": { /* or null if disabled */ },
  "churn": { /* … */ },
  "change_coupling": { /* pairs[].direction = "symmetric" | "one_way" */ },
  "duplication": { /* … */ },
  "hotspot": { /* … */ },
  "lcom": { /* classes[].cluster_count, clusters[].methods */ },
  "severity_counts": { "critical": 2, "high": 5, "medium": 12, "ok": 84 },
  "codebase_files": 142,
  "delta": { /* SnapshotDelta, or null on the first snapshot */ }
}
```

`delta` summarises what changed since the previous snapshot. The
post-commit nudge does not consume it — Severity is computed from the
current `Finding` set against `calibration.toml`.

### `logs/` — event timeline

Lightweight records, no metric payloads. `heal logs` reads them.

| Event type | Written when                                 |
| ---------- | -------------------------------------------- |
| `commit`   | A `git commit` landed (CommitInfo metadata)  |
| `edit`     | Claude edited a file (PostToolUse hook)      |
| `stop`     | A Claude turn ended (Stop hook)              |

`commit` events carry only metadata (sha, parent, author, message
summary, files changed); the full metric payload stays in
`snapshots/`. That split keeps timeline queries fast regardless of
how many metrics are enabled.

### `checks/` — the result cache

The TODO list `/heal-fix` consumes. `heal check` is the only writer.

```json
{
  "version": 1,
  "check_id": "01HKM3Q6Z1B7…",          // ULID
  "started_at": "2026-04-30T05:14:22Z",
  "head_sha": "a0a6d1a…",
  "worktree_clean": true,
  "config_hash": "9f8e7d6c5b4a3210",     // FNV-1a over config + calibration
  "severity_counts": { … },
  "findings": [ /* Vec<Finding> */ ]
}
```

`heal check` short-circuits when `(head_sha, config_hash,
worktree_clean=true)` matches the latest cached record — re-running on
the same commit is free.

`fixed.jsonl` and `regressed.jsonl` live in the same directory but
are flat JSON-lines (not the `EventLog` envelope). They're small,
single-purpose audit trails:

```jsonl
{"finding_id":"ccn:src/payments/engine.ts:processOrder:9f8e…","commit_sha":"a1b2c3","fixed_at":"…"}
```

When a previously-fixed `finding_id` re-appears in a new `heal check`,
the entry moves from `fixed.jsonl` to `regressed.jsonl` and the
renderer warns.

You can inspect any of these streams with standard Unix tools:

```sh
# last 5 commit events from logs/
heal logs --filter commit --limit 5

# walk every CheckRecord
heal cache log --json | jq '.[].check_id'

# raw cache record by id
heal cache show <check_id> --json
```

## Calibration

`calibration.toml` carries the codebase-relative percentile breaks
for every Severity-aware metric. `heal init` computes it from the
initial scan; `heal calibrate --force` refreshes it on demand. The
post-commit nudge reads it through `Calibration::with_overrides(config)`
so any `floor_critical` set in `config.toml` wins over the calibrated
percentile.

Recalibration is **never automatic**. The default `heal calibrate`
invocation evaluates the auto-detect triggers (90-day age, ±20%
codebase file count, 30 days of zero Critical findings) and prints a
recommendation; the user runs `heal calibrate --force` when ready.

The audit trail lives in `.heal/snapshots/` as
`event = "calibrate"`. `MetricsSnapshot::latest_in_segments`
silently skips records that don't decode as a snapshot, so the two
event shapes coexist without interfering.
