---
title: Configuration
description: How to read and edit .heal/config.toml — every section explained, with one realistic example.
---

`heal init` writes `.heal/config.toml`. Everything HEAL needs has a
sensible default, so the first install works with no edits. You only
open the file when you want to tune thresholds for your project.

## Where it lives

```
<your-repo>/.heal/config.toml
```

There is no global config. HEAL reads this file fresh on every
command — no daemon to restart.

## A realistic example

```toml
[project]
response_language = "Japanese"

[git]
since_days = 90
exclude_paths = ["dist/", "vendor/", "node_modules/"]

[metrics]
top_n = 5

[metrics.duplication]
enabled = true
min_tokens = 60

[metrics.hotspot]
enabled = true
weight_churn = 1.0
weight_complexity = 1.5

[policy.complexity_spike]
cooldown_hours = 24
```

You only write the parts you want to override — everything else
falls back to defaults.

## What is on by default

| Metric           | Default                      |
| ---------------- | ---------------------------- |
| LOC              | always on (it has no toggle) |
| Churn            | on                           |
| Cognitive        | on                           |
| CCN (complexity) | off                          |
| Duplication      | off                          |
| Change Coupling  | off                          |
| Hotspot          | off                          |

A "disabled" metric is skipped entirely — its observer never runs
and it never appears in `heal status`. Enable one by setting
`enabled = true` in its section.

## `[project]`

Project-level metadata.

```toml
[project]
response_language = "Japanese"
```

- `response_language` — free-form language hint passed to `heal
check`. Use anything Claude understands: `"Japanese"`, `"日本語"`,
  `"français"`, even `"plain English"`. Optional; if unset, Claude
  picks its default.

## `[git]`

Used by every metric that walks git history (churn, change coupling,
hotspot).

```toml
[git]
since_days = 90
exclude_paths = ["dist/"]
```

- `since_days` (default `90`) — how far back to look. Commits older
  than this are ignored.
- `exclude_paths` (default `[]`) — list of path **substrings** to
  ignore. `"dist/"` matches `packages/web/dist/foo.js` and
  `apps/api/dist/bar.js` alike. There is no glob support; pick a
  specific substring if you need precision.

The LOC observer inherits this list by default. Other observers
always respect it.

## `[metrics]`

Top-level: shared knobs across observers.

```toml
[metrics]
top_n = 5
```

- `top_n` (default `5`) — default size for every "worst-N" listing.
  Each observer has its own `top_n` you can override.

The per-observer subsections below all share two patterns:

- `enabled` — master toggle (LOC has none; it is always on).
- `top_n` (optional) — override the global default for that
  observer's ranking.

### `[metrics.loc]`

```toml
[metrics.loc]
inherit_git_excludes = true
exclude_paths = []
```

- `inherit_git_excludes` (default `true`) — combine with
  `git.exclude_paths`. Set to `false` to detach.
- `exclude_paths` — LOC-only path substrings.

### `[metrics.churn]`

```toml
[metrics.churn]
enabled = true
```

Window length comes from `git.since_days`.

### `[metrics.ccn]` and `[metrics.cognitive]`

These configure the complexity observer. `ccn` (Cyclomatic) is off
by default; `cognitive` (Sonar-style) is on.

```toml
[metrics.ccn]
enabled = true
warn_delta_pct = 30

[metrics.cognitive]
enabled = true
```

- `ccn.warn_delta_pct` (default `30`) — percent change in `max_ccn`
  that fires the SessionStart "complexity spike" rule.

### `[metrics.duplication]`

```toml
[metrics.duplication]
enabled = true
min_tokens = 50
```

- `min_tokens` (default `50`) — minimum window length for a duplicate
  block. Lower → more, smaller blocks.

### `[metrics.change_coupling]`

```toml
[metrics.change_coupling]
enabled = true
min_coupling = 3
```

- `min_coupling` (default `3`) — pairs that co-changed less often
  than this are dropped before ranking.

### `[metrics.hotspot]`

```toml
[metrics.hotspot]
enabled = true
weight_churn = 1.0
weight_complexity = 1.0
```

- `weight_churn` and `weight_complexity` (both default `1.0`) — the
  composed score is `(weight_complexity × ccn_sum) × (weight_churn ×
commits)`. Set either to `0.0` to disable that side without
  touching the underlying observer.

## `[policy.<rule_id>]`

One block per rule. Rules drive the SessionStart nudge — when a
threshold breaches, the rule fires and Claude sees a friendly
"heads-up" at the top of the next session.

```toml
[policy.complexity_spike]
action = "report-only"
cooldown_hours = 24
threshold = { ccn = 15, delta_pct = 20 }
```

- `action` — one of `report-only`, `notify`, `propose`, `execute`.
  Today only `report-only` does anything (the others light up in
  v0.2 with `heal run`).
- `cooldown_hours` (default `24`) — minimum hours between firings of
  the same rule.
- `threshold` — rule-specific thresholds. Keys depend on the rule.

The five rules HEAL evaluates at session start:

| Rule id                        | Fires when                                    |
| ------------------------------ | --------------------------------------------- |
| `hotspot.new_top`              | Top hotspot file changes identity             |
| `complexity.new_top_ccn`       | Top CCN function changes identity             |
| `complexity.new_top_cognitive` | Top Cognitive function changes identity       |
| `complexity.spike`             | `max_ccn` jumps by more than `warn_delta_pct` |
| `duplication.growth`           | Duplicate token count grows                   |

You do not have to declare a rule to use it. Missing entries inherit
defaults (`action = "report-only"`, `cooldown_hours = 24`).

## Strict by design

Every section uses `deny_unknown_fields`. If you mistype a key, HEAL
errors out at startup instead of silently dropping the setting. That
trade-off is intentional — silent drops are how config drift slips
into production.

```toml
[metrics]
typo_n = 5     # ✘ unknown field — heal will error here
```

When you see a parse error, the file path and line number in the
message point at the typo.
