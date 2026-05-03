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
                                  │       (one run_all pass; report consumed below)
                                  │
                                  └──►  stdout: Severity nudge
                                         (Critical / High findings only)

user: heal status (or `claude /heal-code-patch`)
    │
    ▼
heal status  ──►  classify Findings via calibration.toml
                       │
                       ├──►  .heal/findings/latest.json
                       │       (FindingsRecord — the TODO list)
                       │
                       ├──►  reconcile fixed.json ↔ regressed.jsonl
                       │
                       └──►  render Severity-grouped view to stdout
```

`heal` is a single binary; both paths go through it. There is no
daemon, no scheduler, no background process, and no historical
record stream. The post-commit hook runs every observer **once**,
prints the nudge, and exits — nothing is persisted.

## On-disk layout

After `heal init`:

```
<your-repo>/
├── .heal/
│   ├── .gitignore                 # auto — excludes findings/
│   ├── config.toml                # you edit this (tracked in git)
│   ├── calibration.toml           # auto — heal init / heal calibrate (tracked in git)
│   └── findings/
│       ├── latest.json            # current FindingsRecord (TODO list)
│       ├── fixed.json             # BTreeMap<finding_id, FixedFinding> — bounded
│       └── regressed.jsonl        # append-only audit trail of re-detected fixes
│
├── .git/hooks/post-commit         # one-line shim: calls `heal hook commit`
│
└── .claude/skills/                # Claude skills (after `heal skills install`)
    ├── heal-cli/
    ├── heal-code-patch/
    ├── heal-code-review/
    └── heal-config/
```

`config.toml` and `calibration.toml` stay tracked in git so the
team shares the same Severity ladder. `findings/` is excluded by
`.heal/.gitignore`.

## What gets written and when

| File / dir                       | Written by                                         | When                                        |
| -------------------------------- | -------------------------------------------------- | ------------------------------------------- |
| `.heal/.gitignore`               | `heal init`                                        | Once at setup.                              |
| `.heal/config.toml`              | `heal init`                                        | Once at setup; you can edit it freely.      |
| `.heal/calibration.toml`         | `heal init` / `heal calibrate`                     | At setup, then on explicit recalibration.   |
| `.heal/findings/latest.json`     | `heal status`                                      | Each fresh `heal status` (cache-miss path). |
| `.heal/findings/fixed.json`      | `heal mark fix` (called by `/heal-code-patch`)     | Each commit `/heal-code-patch` lands.       |
| `.heal/findings/accepted.json`   | `heal mark accept` (called by `/heal-code-review`) | When the team accepts an intrinsic finding. |
| `.heal/findings/regressed.jsonl` | `heal status` (reconcile pass)                     | When a fixed finding is re-detected.        |
| `.claude/skills/heal-*/`         | `heal skills install`                              | Once; updated with `heal skills update`.    |

There is no event log, no monthly rotation, no `.heal/snapshots/`,
`.heal/logs/`, `.heal/docs/`, or `.heal/reports/` directory. heal
keeps only the current state plus the small audit trail in
`regressed.jsonl`.

## The findings cache

`.heal/findings/` holds four artifacts; `heal status` is the only
writer of `latest.json` and `regressed.jsonl`, `heal mark fix` is
the only writer of `fixed.json`, and `heal mark accept` is the only
writer of `accepted.json`.

### `latest.json` — the current TODO

```json
{
  "version": 2,
  "id": "01HKM3Q6Z1B7…", // ULID
  "started_at": "2026-04-30T05:14:22Z",
  "head_sha": "a0a6d1a…",
  "worktree_clean": true,
  "config_hash": "9f8e7d6c5b4a3210", // FNV-1a over config + calibration
  "severity_counts": { "critical": 2, "high": 5, "medium": 12, "ok": 84 },
  "findings": [
    /* Vec<Finding> */
  ]
}
```

`heal status` short-circuits when `(head_sha, config_hash,
worktree_clean=true)` matches the cached record — re-running on the
same commit is free.

### `fixed.json` — bounded fix record

A `BTreeMap<finding_id, FixedFinding>` serialized as a single JSON
object. Each entry is keyed by the deterministic `finding_id`:

```json
{
  "ccn:src/payments/engine.ts:processOrder:9f8e…": {
    "commit_sha": "a1b2c3",
    "fixed_at": "2026-04-30T05:14:22Z"
  }
}
```

Bounded — never append-only. When a previously-fixed `finding_id`
re-appears in a new `heal status`, heal moves it out of `fixed.json`
and writes a line to `regressed.jsonl`; the renderer warns.

### `regressed.jsonl` — the audit trail

The only append-only file in `.heal/`. One JSON line per regression
event, used solely for the "fix was re-detected" warning surface.

You can inspect the cache directly with `jq`:

```sh
jq '.severity_counts' .heal/findings/latest.json
jq 'keys | length'    .heal/findings/fixed.json
tail .heal/findings/regressed.jsonl
```

## Calibration

`calibration.toml` carries the codebase-relative percentile breaks
for every Severity-aware metric. `heal init` computes it from the
initial scan; `heal calibrate --force` refreshes it on demand.
`floor_critical` / `floor_ok` set in `config.toml` win over the
calibrated percentile. Recalibration is **never automatic** — see
[CLI › `heal calibrate`](/heal/cli/#heal-calibrate).

## Calibration vs policy: two layers

heal separates the _measurement_ of code health from the _intent_ of
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

| Tier                  | Default specs                           | Renderer behavior                      | Skill behavior                                    |
| --------------------- | --------------------------------------- | -------------------------------------- | ------------------------------------------------- |
| **T0 / Drain queue**  | `must = ["critical:hotspot"]`           | Always shown, sorted Severity 🔥 desc. | `/heal-code-patch` drains one finding per commit. |
| **T1 / Should drain** | `should = ["critical", "high:hotspot"]` | Shown by default, separate section.    | Surfaced for review; not auto-drained.            |
| **Advisory**          | everything else above Ok                | Hidden unless `--all`.                 | Never drained; review when convenient.            |

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
