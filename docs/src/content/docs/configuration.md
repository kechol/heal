---
title: Configuration
description: How to read and edit .heal/config.toml — every section explained, with one realistic example.
---

`heal init` writes `.heal/config.toml`. Every heal setting has a
sensible default, so the initial install works without edits. Edit
the file only to override defaults for the project.

## File location

```
<your-repo>/.heal/config.toml
```

The configuration is per-repository; there is no global config. heal
re-reads the file on every invocation — no daemon to restart.

## Example configuration

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

Only the overridden values need to be written; the rest falls back
to defaults.

## Defaults

| Metric           | Default                    |
| ---------------- | -------------------------- |
| LOC              | always enabled (no toggle) |
| Churn            | enabled                    |
| Cognitive        | enabled                    |
| CCN (complexity) | disabled                   |
| Duplication      | disabled                   |
| Change Coupling  | disabled                   |
| Hotspot          | disabled                   |

A disabled metric is skipped entirely: its observer does not run, and
it does not appear in `heal status`. Enable a metric by setting
`enabled = true` in its section.

## `[project]`

Project-level metadata.

```toml
[project]
response_language = "Japanese"
```

- `response_language` — free-form language hint passed to `heal
check`. Any value Claude understands is acceptable: `"Japanese"`,
  `"日本語"`, `"français"`, `"plain English"`. Optional; when unset,
  Claude uses its default.

## `[git]`

Used by every metric that walks git history (churn, change coupling,
hotspot).

```toml
[git]
since_days = 90
exclude_paths = ["dist/"]
```

- `since_days` (default `90`) — lookback window. Commits older than
  this are ignored.
- `exclude_paths` (default `[]`) — list of path **substrings** to
  ignore. `"dist/"` matches both `packages/web/dist/foo.js` and
  `apps/api/dist/bar.js`. Glob patterns are not supported; use a
  specific substring when precision is required.

The LOC observer inherits this list by default. Other observers always
respect it.

## `[metrics]`

Top-level fields are shared across observers.

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

These configure the complexity observer. `ccn` (Cyclomatic) is
disabled by default; `cognitive` (Sonar-style) is enabled.

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
  block. Lower values surface more, shorter blocks.

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
commits)`. Setting either to `0.0` disables that side of the
  composition without disabling the underlying observer.

## `[policy.<rule_id>]`

One block per rule. Rules drive the SessionStart nudge: when a
threshold breaches, the rule fires and a notice appears at the top
of the next Claude session.

```toml
[policy.complexity_spike]
action = "report-only"
cooldown_hours = 24
threshold = { ccn = 15, delta_pct = 20 }
```

- `action` — one of `report-only`, `notify`, `propose`, `execute`.
  In v0.1 only `report-only` is active; the other actions become
  meaningful in v0.2 alongside `heal run`.
- `cooldown_hours` (default `24`) — minimum hours between two firings
  of the same rule.
- `threshold` — rule-specific thresholds. Keys depend on the rule.

The five rules heal evaluates at session start:

| Rule id                        | Fires when                                    |
| ------------------------------ | --------------------------------------------- |
| `hotspot.new_top`              | Top hotspot file changes identity             |
| `complexity.new_top_ccn`       | Top CCN function changes identity             |
| `complexity.new_top_cognitive` | Top Cognitive function changes identity       |
| `complexity.spike`             | `max_ccn` jumps by more than `warn_delta_pct` |
| `duplication.growth`           | Duplicate token count grows                   |

Rules do not need explicit declaration; missing entries inherit the
defaults (`action = "report-only"`, `cooldown_hours = 24`).

## Strict by design

Every section uses `deny_unknown_fields`. A misspelled key produces a
parse error at startup rather than being silently dropped. The
trade-off is intentional: silent drops are a common path for config
drift to reach production.

```toml
[metrics]
typo_n = 5     # ✘ unknown field — heal errors at this line
```

Parse errors include the file path and line number of the offending
key.
