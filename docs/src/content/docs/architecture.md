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

user: heal status (or `/heal:refactor` in Claude Code)
    │
    ▼
heal status  ──►  classify Findings via calibration.toml
                       │
                       ├──►  .heal/findings/latest.json
                       │       (FindingsRecord — the TODO list)
                       │
                       ├──►  reconcile fixed.json ↔ regressed.jsonl
                       │
                       └──►  render Tier/Severity/score-ordered view to stdout
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
│   ├── config.toml                # you edit this (tracked in git)
│   ├── calibration.toml           # auto — heal init / heal calibrate (tracked in git)
│   └── findings/                  # tracked in git — team shares the TODO
│       ├── latest.json            # current FindingsRecord (TODO list)
│       ├── fixed.json             # bounded record of fixes
│       ├── accepted.json          # "won't fix / intrinsic" lane
│       └── regressed.jsonl        # append-only audit trail of re-detected fixes
│
└── .git/hooks/post-commit         # one-line shim: calls `heal hook commit`
```

The Claude skills are not stored in the repository. They come from the
heal Claude Code plugin, which Claude Code keeps in its own settings
directory, so installing or updating them never touches your git tree.
heal 0.6 and earlier copied them into `.claude/skills/heal-*` and
`.agents/skills/heal-*`; `heal skills uninstall` removes those copies.

`config.toml`, `calibration.toml`, and the `findings/` directory
are all tracked in git. Teammates on the same commit share the
same Severity ladder and the same drain queue.

## What gets written and when

| File / dir                       | Written by                                                                | When                                                       |
| -------------------------------- | ------------------------------------------------------------------------- | ---------------------------------------------------------- |
| `.heal/config.toml`              | `heal init`                                                               | Once at setup; you can edit it freely.                     |
| `.heal/calibration.toml`         | `heal init` / `heal calibrate`                                            | At setup, then on explicit recalibration.                  |
| `.heal/findings/latest.json`     | `heal status`                                                             | Each fresh `heal status` (cache-miss path).                |
| `.heal/findings/fixed.json`      | `heal mark fix` (called by `/heal:refactor`, `/heal:docs`, `/heal:tests`) | Each commit one of those skills lands.                     |
| `.heal/findings/accepted.json`   | `heal mark accept` (called by the same skills, after you approve)         | When the team accepts an intrinsic finding.                |
| `.heal/findings/regressed.jsonl` | `heal status` (reconcile pass)                                            | When a fixed finding is re-detected.                       |
| `.heal/doc_pairs.json`           | `/heal:setup` skill (when `[features.docs]` is on)                        | When the user runs the skill; HEAL is read-only.           |
| `.heal/concepts.toml`            | `/heal:setup` skill (when `[features.semantic]` is on)                    | When the user runs the skill or edits the vocabulary.      |
| `.heal/semantic/verdicts/`       | `heal semantic ask`                                                       | Each run that asks something new; commit it with the code. |
| `.heal/cache/source-v1.json`     | source observers                                                          | After source analysis changes; safe to delete.             |
| `.heal/cache/semantic/verdicts/` | `heal semantic ask` (on-demand checks, `focus`)                           | Each such run; safe to delete.                             |

There is no event log, no monthly rotation, no `.heal/snapshots/`,
`.heal/logs/`, or `.heal/reports/` directory. heal keeps only the
current state plus the small audit trail in `regressed.jsonl`.

`.heal/cache/` is different from the tracked findings cache. It contains only
disposable data — Complexity, LCOM, and Duplication token data for unchanged
source files, and the answers of the semantic checks that concern one person's
work — and ignores itself in git. heal validates file contents before every
reuse; deleting the directory, a corrupt entry, or a write failure simply
causes normal source analysis.

`.heal/docs/` is the one exception: when `/heal:docs scaffold` runs,
it writes the generated documentation tree to the directory named by
`[features.docs] scaffold_root` (default `.heal/docs/`). HEAL
itself only ever **reads** that tree — the skill is the only writer.

## The findings cache

`.heal/findings/` holds four artifacts; `heal status` is the only
writer of `latest.json` and `regressed.jsonl`, `heal mark fix` is
the only writer of `fixed.json`, and `heal mark accept` is the only
writer of `accepted.json`.

### `latest.json` — the current TODO

```json
{
  "version": 9,
  "id": "9f8e7d6c5b4a3210", // FNV-1a hex of (head_sha, config_hash, worktree_clean)
  "head_sha": "a0a6d1a…",
  "worktree_clean": true,
  "config_hash": "9f8e7d6c5b4a3210", // rules + enabled non-git observations
  "severity_counts": { "critical": 2, "high": 5, "medium": 12, "ok": 84 },
  "coverage_observation": {
    "state": "partial",
    "configured_sources": ["lcov.info"],
    "sources": ["lcov.info"],
    "unreadable_sources": [],
    "unmeasured_files": ["src/new.ts"]
  },
  "findings": [/* Vec<Finding> */]
}
```

`heal status` short-circuits when `(head_sha, config_hash,
worktree_clean=true)` matches the cached record. `config_hash` includes
`config.toml`, `calibration.toml`, and each enabled LCOV/doc-pair
logical path, readable/missing state, and content. It excludes mtimes,
absolute checkout paths, and disabled-family inputs. Consequently, the
same HEAD is not enough to reuse a result when an ignored LCOV report
or doc-pair file changed, while identical observations remain
byte-reproducible across checkouts. Historical windows use the observed
HEAD/ref commit timestamp rather than the wall clock.

Schema v6 introduced that broader observation hash, v7 added coverage
provenance, and v8 added optional `findings[].hotspot_score`. Old cache
versions are discarded and rebuilt automatically. A production file
absent from LCOV is reported as unmeasured, not synthesized as 0%; an
explicit LCOV 0% record remains a measured finding.

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

### `accepted.json` — the "won't fix / intrinsic" lane

A `BTreeMap<finding_id, AcceptedFinding>` serialized as a single JSON
object, written by `heal mark accept` (called by `/heal:refactor`,
`/heal:docs`, or `/heal:tests` when the team approves recording a
finding as intrinsic so it isn't drained).

```json
{
  "ccn:src/payments/engine.ts:processOrder:9f8e…": {
    "reason": "intrinsic — branchy by design (tax engine)",
    "file": "src/payments/engine.ts",
    "metric": "ccn",
    "severity": "critical",
    "hotspot": true,
    "metric_value": 31.0,
    "summary": "CCN=31 processOrder (TypeScript)",
    "accepted_at": "2026-04-30T05:14:22Z",
    "accepted_by": "Alice <alice@example.com>"
  }
}
```

Distinct from `fixed.json` — accepted entries are not consumed when
the finding re-appears. They suppress the finding's drain-queue
presence indefinitely, and `heal status` shows them in a separate
`Accepted: N findings` header line plus a `📌 Accepted` section
under `--all`.

`Finding.accepted: bool` is decorated at render time by folding
`accepted.json` into the finding list — `latest.json` itself keeps
raw observer truth and never carries `accepted: true`. That way
toggling acceptance doesn't require a rescan.

Removing an entry: hand-edit the file to drop the row, or call
`heal mark accept --remove` from the skill flow. The next
`heal status` will surface the underlying finding again.

If an accepted finding's Severity rises or its family Hotspot changes
from false to true, status, current-side diff, and the post-commit hook
request re-review. JSON exposes one `accepted_rereview` object with one
or both reasons (`severity_increased`, `became_hotspot`). Acceptance is
not removed and the finding is not returned to T0. Stable, cooling, and
improving accepted findings stay quiet.

You can inspect the cache directly with `jq`:

```sh
jq '.severity_counts' .heal/findings/latest.json
jq 'keys | length'    .heal/findings/fixed.json
jq 'keys | length'    .heal/findings/accepted.json
tail .heal/findings/regressed.jsonl
```

## Calibration

`calibration.toml` carries the codebase-relative percentile breaks
for every Severity-aware metric. `heal init` computes it from the
initial scan; `heal calibrate --force` refreshes it on demand.
`floor_critical` / `floor_ok` set in `config.toml` win over the
calibrated percentile. Recalibration is **never automatic** — see
[CLI › `heal calibrate`](/heal/cli/#heal-calibrate).

Hotspot uses p90 plus the family floor with 5+ finite candidates. With
1–4 it uses the existing absolute family floor alone (Code 22, Test 25,
Docs 5); non-finite values never flag. This fallback is not a percentile
claim, probability estimate, or guarantee of remediation payoff.

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

| Tier                  | Default specs                           | Renderer behavior                             | Skill behavior                                               |
| --------------------- | --------------------------------------- | --------------------------------------------- | ------------------------------------------------------------ |
| **T0 / Drain queue**  | `must = ["critical:hotspot"]`           | Always shown in deterministic priority order. | Proposed first; applied one proposal per commit on approval. |
| **T1 / Should drain** | `should = ["critical", "high:hotspot"]` | Shown by default, separate section.           | Proposed after T0.                                           |
| **Advisory**          | everything else above Ok                | Hidden unless `--all`.                        | Context only; never proposed on its own.                     |

Findings classified as `Severity::Ok` are excluded from drain entirely.
With `--all` they appear in the normal Ok section in score order; a
mixed section marks individual hotspot rows with `🔥`. Without
`--all`, they contribute only to the hidden-summary count.

Within each family and Tier, rows sort by Severity and then descending
`hotspot_score`, followed by stable metric/path/id tie-breakers. Scores
from Code, Test, and Docs are never compared with one another.

Override visibility: when `[metrics.<m>] floor_ok` or `floor_critical`
deviates from the literature default, `heal status` emits a header line
like `override: ccn floor_ok=15 [override from 11]` so policy moves are
auditable in CI logs and PR diffs.

The `[policy.drain]` DSL is `<severity>` (any hotspot) or
`<severity>:hotspot` (hotspot=true required). Severity tokens are
lowercase: `critical / high / medium / ok`. See
[Code › Configuration › `[policy.drain]`](/heal/code/configuration/#policydrain).
