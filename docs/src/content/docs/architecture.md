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

user: heal status (or `claude /heal-code-patch`)
    │
    ▼
heal status  ──►  classify Findings via calibration.toml
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
│       ├── fixed.jsonl            # `/heal-code-patch` claimed a commit fixes a finding
│       └── regressed.jsonl        # a fix re-detected — surfaced as a warning
│
├── .git/hooks/post-commit         # one-line shim: calls `heal hook commit`
│
├── .claude/skills/                # Claude skills (after `heal skills install`)
│   ├── heal-cli/
│   ├── heal-code-patch/
│   ├── heal-code-review/
│   └── heal-config/
│
└── .claude/settings.json          # PostToolUse + Stop hooks call `heal hook edit/stop`
```

## What gets written and when

| File / dir                      | Written by                                   | When                                         |
| ------------------------------- | -------------------------------------------- | -------------------------------------------- |
| `.heal/config.toml`             | `heal init`                                  | Once at setup; you can edit it freely.       |
| `.heal/calibration.toml`        | `heal init` / `heal calibrate`               | At setup, then on explicit recalibration.    |
| `.heal/snapshots/YYYY-MM.jsonl` | post-commit hook + `heal calibrate`          | On every `git commit` and recalibration.     |
| `.heal/logs/YYYY-MM.jsonl`      | post-commit + Claude PostToolUse / Stop      | On every commit and Claude tool event.       |
| `.heal/checks/YYYY-MM.jsonl`    | `heal status`                                 | Each fresh `heal status` (cache-miss path).   |
| `.heal/checks/latest.json`      | `heal status`                                 | Atomic mirror; refreshed on every fresh run. |
| `.heal/checks/fixed.jsonl`      | `heal mark-fixed` (called by `/heal-code-patch`) | Each commit `/heal-code-patch` lands.          |
| `.heal/checks/regressed.jsonl`  | `heal status` (reconcile pass)                | When a fixed finding is re-detected.         |
| `.claude/skills/heal-*/`        | `heal skills install`                        | Once; updated with `heal skills update`.     |
| `.claude/settings.json` (HEAL hooks) | `heal skills install`                   | Additive merge; uninstall removes only HEAL command lines. |
| `.heal/skills-install.json`     | `heal skills install` / `update`             | Drift-detection manifest.                    |

The pre-v0.2 `.heal/state.json` was retired along with the
SessionStart nudge — `EventLog::iter_segments` over `snapshots/` is
now the single way to query historical state.

## The event log

`snapshots/`, `logs/`, and `checks/` share the same on-disk format:

- **One file per month**: `YYYY-MM.jsonl` (UTC).
- **Append-only**: every record is one JSON object on one line.
- **Transparent gzip**: readers handle `.gz` files alongside plain
  text. `heal compact` (also called automatically from
  `heal hook commit`) gzips segments older than 90 days and deletes
  those past 365 days.

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

Written on every commit (`event = "commit"`) and every recalibration
(`event = "calibrate"`). The two co-exist; readers filter by `event`
before decoding.

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
  "change_coupling": {
    /* pairs[].direction = "symmetric" | "one_way" */
  },
  "duplication": {
    /* … */
  },
  "hotspot": {
    /* … */
  },
  "lcom": {
    /* classes[].cluster_count, clusters[].methods */
  },
  "severity_counts": { "critical": 2, "high": 5, "medium": 12, "ok": 84 },
  "codebase_files": 142,
  "delta": {
    /* SnapshotDelta, or null on the first snapshot */
  }
}
```

`delta` summarises what changed since the previous snapshot. The
post-commit nudge does not consume it — Severity is computed from the
current `Finding` set against `calibration.toml`.

### `logs/` — event timeline

Lightweight records, no metric payloads. `heal logs` walks them.

| Event type | Written when                                |
| ---------- | ------------------------------------------- |
| `commit`   | A `git commit` landed (CommitInfo metadata) |
| `edit`     | Claude edited a file (PostToolUse hook)     |
| `stop`     | A Claude turn ended (Stop hook)             |

`commit` events carry only metadata (sha, parent, author, message
summary, files changed); the full metric payload stays in
`snapshots/`. That split keeps timeline queries fast regardless of
how many metrics are enabled.

### `checks/` — the result cache

The TODO list `/heal-code-patch` consumes. `heal status` is the only writer.

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

`heal status` short-circuits when `(head_sha, config_hash,
worktree_clean=true)` matches the latest cached record — re-running on
the same commit is free.

`fixed.jsonl` and `regressed.jsonl` live in the same directory but
are flat JSON-lines (not the `EventLog` envelope). They're small,
single-purpose audit trails:

```jsonl
{
  "finding_id": "ccn:src/payments/engine.ts:processOrder:9f8e…",
  "commit_sha": "a1b2c3",
  "fixed_at": "…"
}
```

When a previously-fixed `finding_id` re-appears in a new `heal status`,
the entry moves from `fixed.jsonl` to `regressed.jsonl` and the
renderer warns.

You can inspect any of these streams with the matching browser:

```sh
# last 5 commit events from logs/
heal logs --filter commit --limit 5

# every MetricsSnapshot + calibrate event
heal snapshots --json --limit 20

# every CheckRecord summary
heal checks --json | jq '.[].check_id'

# raw CheckRecord (full Findings list) by check_id
heal checks --json | jq '.[] | select(.check_id == "<check_id>")'
```

## Calibration

`calibration.toml` carries the codebase-relative percentile breaks
for every Severity-aware metric. `heal init` computes it from the
initial scan; `heal calibrate --force` refreshes it on demand. The
post-commit nudge reads it through `Calibration::with_overrides(config)`
so any `floor_critical` / `floor_ok` set in `config.toml` wins over the
calibrated percentile.

Recalibration is **never automatic**. The default `heal calibrate`
invocation evaluates the auto-detect triggers (90-day age, ±20%
codebase file count, 30 days of zero Critical findings) and prints a
recommendation; the user runs `heal calibrate --force` when ready.

The audit trail lives in `.heal/snapshots/` as
`event = "calibrate"`. `MetricsSnapshot::latest_in_segments`
silently skips records that don't decode as a snapshot, so the two
event shapes coexist without interfering.

## Calibration vs policy: two layers

heal separates the *measurement* of code health from the *intent* of
what to act on:

- **Calibration layer** (`.heal/calibration.toml` + per-metric
  `[metrics.<m>]` overrides) decides "is this finding red?". The
  three-way classifier — `floor_critical` (escape hatch) +
  `floor_ok` (graduation gate, proxy metrics only) + percentile
  breaks — produces a Severity. This layer answers a measurement
  question: where does this value sit relative to literature
  thresholds and the project's own distribution.
- **Policy layer** (`[policy.drain]` in `config.toml`) decides "is
  this finding actionable?". A `(Severity, hotspot)` tuple maps to
  one of three drain tiers: T0 / `must`, T1 / `should`, or
  Advisory. This layer answers an intent question: what does the
  team commit to draining.

The two layers are orthogonal — re-calibration shifts where Severity
boundaries fall but never touches policy; a stricter or looser policy
changes drain semantics without re-running observers. Teams typically
keep calibration close to literature defaults and tune `[policy.drain]`
for their bandwidth.

## Drain queue model

`heal status` partitions every non-Ok finding into one of three
buckets driven by `[policy.drain]`:

| Tier | Default specs | Renderer behaviour | Skill behaviour |
| --- | --- | --- | --- |
| **T0 / Drain queue** | `must = ["critical:hotspot"]` | Always shown, sorted Severity 🔥 desc. | `/heal-code-patch` drains one finding per commit. |
| **T1 / Should drain** | `should = ["critical", "high:hotspot"]` | Shown by default, separate section. | Surfaced for review; not auto-drained. |
| **Advisory** | everything else above Ok | Hidden unless `--all`. | Never drained; review when convenient. |

Findings classified as `Severity::Ok` are excluded from drain entirely;
the renderer surfaces them via a dedicated Ok 🔥 pre-section (top-10%
hotspot but below the metric floor) and a hidden-summary count.

Override visibility: when `[metrics.<m>] floor_ok` or `floor_critical`
deviates from the literature default, `heal status` emits a header line
like `override: ccn floor_ok=15 [override from 11]` so policy moves are
auditable in CI logs and PR diffs.

The `[policy.drain]` DSL is `<severity>` (any hotspot) or
`<severity>:hotspot` (hotspot=true required). Severity tokens are
lowercase: `critical / high / medium / ok`. See
[Configuration › Drain policy](/heal/configuration/#drain-policy).
