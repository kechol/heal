# `.heal/config.toml` — complete reference

Authoritative reference for every key in `.heal/config.toml`. Loaded by
the `heal-config` skill when it needs to justify a setting, and by
`heal-code-review` when a finding's "fix" is to relax a threshold.

The file lives at `<project>/.heal/config.toml`. It is parsed by
`heal-cli`'s `Config::load`, which is **strict** — `deny_unknown_fields`
is on for every section. Typos surface as schema errors, not silent
defaults.

When a section is **omitted** from TOML, the schema applies the
"missing-section default". Every metric's missing-section default is
`enabled = true` (so a fresh `config.toml` doesn't have to enumerate
every observer); programmatic `Default::default()` produces the same
struct, and there's a regression test pinning the two paths together.

```
.heal/config.toml
├── [project]      — language hints, project-level metadata
├── [git]          — repo-wide observer scope (since-days, exclude paths)
├── [metrics]      — top-N width + per-metric toggles & thresholds
│   ├── [metrics.loc]              — foundational; no enable toggle
│   ├── [metrics.churn]
│   ├── [metrics.hotspot]
│   ├── [metrics.change_coupling]
│   ├── [metrics.duplication]
│   ├── [metrics.ccn]
│   ├── [metrics.cognitive]
│   └── [metrics.lcom]
└── [policy]
    ├── [policy.drain]                            — global must / should
    ├── [policy.drain.metrics.<name>]             — per-metric overrides
    └── [policy.rules.<name>]                     — reserved (parse-only)
```

## `[project]`

Project-level metadata that doesn't fit anywhere else. Currently a
single optional key.

| Key                  | Type               | Default | Meaning                                                                                                                                                                                |
|----------------------|--------------------|---------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `response_language`  | `Option<String>`   | `None`  | Natural language for AI-generated output (heal's own renderings, future `run-*` proposals). Free-form: `"Japanese"`, `"日本語"`, `"ja"`, `"français"`. `None` keeps the model default. |

`response_language` only governs heal's own output. Skills like
`heal-code-review` follow their own language convention — it does not
propagate to them.

## `[git]`

Where heal looks in the git history and which paths every observer
should ignore.

| Key             | Type           | Default       | Meaning                                                                                                  |
|-----------------|----------------|---------------|----------------------------------------------------------------------------------------------------------|
| `since_days`    | `u32`          | `90`          | Window for the churn / change-coupling observers. Older commits are ignored.                             |
| `exclude_paths` | `Vec<String>`  | `[]`          | Path prefixes to skip in *every* observer that respects git excludes (LOC inherits this when its toggle is on; observers that pull from LOC excludes inherit too). One entry per directory; trailing `/` is fine but not required. |

`since_days = 90` matches the calibration window — extending it pulls
older commits into churn/coupling but doesn't change Severity (Severity
is calibrated from the *current* file distribution).

`exclude_paths` is the canonical project-wide exclude set. Use this
rather than per-metric `exclude_paths` whenever you can — it propagates
via `metrics.loc.inherit_git_excludes = true` to every observer that
honours LOC's exclude list. Per-metric `exclude_paths` exists for the
narrow case where one observer should skip a path the others should
still measure.

## `[metrics]`

Top-level metric controls and per-metric sections.

| Key      | Type    | Default | Meaning                                                                                                                              |
|----------|---------|---------|--------------------------------------------------------------------------------------------------------------------------------------|
| `top_n`  | `usize` | `5`     | Default `worst_n` width for `heal metrics` rankings. Each metric below can override this with its own `top_n`; absent overrides fall back here. |

### `[metrics.loc]` — Lines of Code

Foundational metric. **No enable toggle** — other observers (hotspot,
churn weighting, primary-language detection) depend on it.

| Key                     | Type             | Default | Meaning                                                                                                                                          |
|-------------------------|------------------|---------|--------------------------------------------------------------------------------------------------------------------------------------------------|
| `inherit_git_excludes`  | `bool`           | `true`  | Add `git.exclude_paths` to LOC's exclude set. Leave on; turning off requires duplicating the list.                                              |
| `exclude_paths`         | `Vec<String>`    | `[]`    | LOC-specific extra excludes. Populated mostly when one observer should skip a path the others measure (rare).                                    |
| `top_n`                 | `Option<usize>`  | `None`  | Override `metrics.top_n` for the LOC top-languages list.                                                                                        |

### `[metrics.churn]`

Commits-touching-file count over the `since_days` window. Useful as
the heat half of the hotspot composition; on its own a noisy signal.

| Key       | Type             | Default | Meaning                                              |
|-----------|------------------|---------|------------------------------------------------------|
| `enabled` | `bool`           | `true`  | Disable on solo-author repos with no shared history. |
| `top_n`   | `Option<usize>`  | `None`  | Override the most-churned files list width.          |

### `[metrics.hotspot]` — composition of CCN × Churn

Flag, not a Severity. Files in the top 10% by composed score get
`hotspot=true` on their findings; the drain policy uses this to gate
T0 vs Advisory.

| Key                  | Type             | Default | Meaning                                                                                                                                |
|----------------------|------------------|---------|----------------------------------------------------------------------------------------------------------------------------------------|
| `enabled`            | `bool`           | `true`  | Off only on tiny repos where complexity and churn agree trivially.                                                                     |
| `weight_churn`       | `f64`            | `1.0`   | Relative weight of churn in the geometric-mean composition.                                                                            |
| `weight_complexity`  | `f64`            | `1.0`   | Relative weight of complexity. Bumping one above the other amplifies that signal — leave equal unless the calibration shows imbalance. |
| `top_n`              | `Option<usize>`  | `None`  | Override the top-hotspots list width. Also drives the new-in-top-N membership diff in snapshots.                                       |

### `[metrics.change_coupling]`

Pairs of files that change together more often than chance.

| Key                    | Type             | Default | Meaning                                                                                                                                                          |
|------------------------|------------------|---------|------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `enabled`              | `bool`           | `true`  | Disable on repos with no meaningful shared edits.                                                                                                                |
| `min_coupling`         | `u32`            | `3`     | Scan-time floor: pairs with fewer than N co-occurrences drop before classification. Raise on noisy histories; lower on small repos.                              |
| `min_lift`             | `f64`            | `2.0`   | Lift = `P(A∩B) / (P(A)·P(B))`. Pairs below drop as coincidental. `2.0` ≈ "twice chance"; `1.5` is looser; `3.0` is strict.                                       |
| `symmetric_threshold`  | `f64`            | `0.5`   | Both `P(B|A)` and `P(A|B)` must exceed this for a pair to classify as `Symmetric` (rather than `OneWay`).                                                       |
| `top_n`                | `Option<usize>`  | `None`  | Override the most-coupled-pairs list width.                                                                                                                      |
| `floor_critical`       | `Option<f64>`    | `None`  | Absolute Critical floor on the metric value. Rare in practice — `min_coupling` already serves as a floor; leave `None` to defer entirely to percentile breaks.   |

### `[metrics.duplication]`

Token-level near-duplicate detection (FNV-1a fingerprint per token
window).

| Key              | Type             | Default | Meaning                                                                                                          |
|------------------|------------------|---------|------------------------------------------------------------------------------------------------------------------|
| `enabled`        | `bool`           | `true`  |                                                                                                                  |
| `min_tokens`     | `u32`            | `50`    | Smallest duplicate block size, in tokens. Lower → more matches and more false positives; higher → only big copies. |
| `top_n`          | `Option<usize>`  | `None`  | Override the largest-duplicate-blocks list width.                                                                 |
| `floor_critical` | `Option<f64>`    | `None`  | Per-file duplicate-percentage floor. Defaults to `core::calibration::FLOOR_DUPLICATION_PCT` (30%) if `None`.     |

### `[metrics.ccn]` — McCabe cyclomatic complexity

| Key              | Type             | Default | Meaning                                                                                                                                                   |
|------------------|------------------|---------|-----------------------------------------------------------------------------------------------------------------------------------------------------------|
| `enabled`        | `bool`           | `true`  |                                                                                                                                                           |
| `top_n`          | `Option<usize>`  | `None`  | Override the complexity list width (covers both CCN and Cognitive — they share the `complexity:` section in `heal metrics`).                              |
| `floor_critical` | `Option<f64>`    | `None`  | Override the absolute Critical floor. Default: `core::calibration::FLOOR_CCN` = 25 (McCabe's "untestable").                                              |
| `floor_ok`       | `Option<f64>`    | `None`  | Override the Ok graduation gate. Default: `core::calibration::FLOOR_OK_CCN` = 11 (McCabe's "simple, low risk"). Values strictly below classify as Ok regardless of percentile. |

### `[metrics.cognitive]` — Sonar cognitive complexity

| Key              | Type             | Default | Meaning                                                                                                                                                |
|------------------|------------------|---------|--------------------------------------------------------------------------------------------------------------------------------------------------------|
| `enabled`        | `bool`           | `true`  |                                                                                                                                                        |
| `floor_critical` | `Option<f64>`    | `None`  | Override the absolute Critical floor. Default: `core::calibration::FLOOR_COGNITIVE` = 50 (Sonar Critical baseline).                                    |
| `floor_ok`       | `Option<f64>`    | `None`  | Override the Ok graduation gate. Default: `core::calibration::FLOOR_OK_COGNITIVE` = 8 (half of Sonar's "review" threshold). Strictly-below → Ok. |

### `[metrics.lcom]` — Lack of Cohesion of Methods

| Key                 | Type             | Default          | Meaning                                                                                                                          |
|---------------------|------------------|------------------|----------------------------------------------------------------------------------------------------------------------------------|
| `enabled`           | `bool`           | `true`           | Off on repos with no classes. The `tree-sitter-approx` backend silently emits zero findings on such repos either way.            |
| `backend`           | enum             | `tree-sitter-approx` | Extraction backend. `lsp` is reserved for v0.5+ — config opt-in is allowed but the variant doesn't yet drive any analyzer.   |
| `min_cluster_count` | `u32`            | `2`              | Classes whose `cluster_count` is below this floor aren't surfaced as Findings. `1` = cohesive, `0` = no methods, so `2` is the natural baseline. |
| `top_n`             | `Option<usize>`  | `None`           | Override the most-split-classes list width.                                                                                      |
| `floor_critical`    | `Option<f64>`    | `None`           | Absolute Critical floor. Rare — `min_cluster_count` already serves as the scan-time filter.                                      |

## `[policy]`

The drain queue + reserved-for-future user-defined rules.

### `[policy.drain]` — drain tier policy

`heal status` classifies every Finding into a tier:

- **T0 (`must`)** — drain to zero. The `heal-code-patch` skill iterates only over T0.
- **T1 (`should`)** — drain when bandwidth permits. Surfaced separately in renderings.
- **Advisory** — everything else above `Severity::Ok`. Mention as a count, never as TODO entries.

| Key       | Type                                | Default                              | Meaning                                              |
|-----------|-------------------------------------|--------------------------------------|------------------------------------------------------|
| `must`    | `Vec<DrainSpec>`                    | `["critical:hotspot"]`               | T0 list. The drain skill targets only these.         |
| `should`  | `Vec<DrainSpec>`                    | `["critical", "high:hotspot"]`       | T1 list. Surface as advisory; never auto-drain.      |
| `metrics` | `BTreeMap<String, MetricOverride>`  | `{}`                                 | Per-metric overrides — see below.                    |

A `DrainSpec` is one entry in the lists, written as a string:

- `<severity>` — match findings of that severity regardless of `hotspot`.
- `<severity>:hotspot` — match only when `hotspot = true`.

Severity tokens: `critical`, `high`, `medium`, `ok`. The only valid
flag is `hotspot`.

### `[policy.drain.metrics.<name>]` — per-metric override

Either field may be omitted to inherit the global list. Useful when
you want `ccn` (proxy metric) stricter than `duplication` (Goodhart-
safe) — or vice versa.

```toml
[policy.drain.metrics.ccn]
must   = ["critical:hotspot", "high:hotspot"]
should = ["critical", "high"]

[policy.drain.metrics.duplication]
must   = ["critical"]
# `should` inherits the global list
```

Sub-metrics (e.g. `change_coupling.symmetric`) fall back to their
parent (`change_coupling`) before the global list, so an override on
`change_coupling` covers both `change_coupling` and
`change_coupling.symmetric`.

### `[policy.rules.<name>]` — reserved

Currently parse-only; reserved for v0.4 metric-drift actions. Each
rule carries:

| Key         | Type                              | Required |
|-------------|-----------------------------------|----------|
| `action`    | one of `report-only`, `notify`, `propose`, `execute` | yes |
| `threshold` | `BTreeMap<String, toml::Value>`   | no       |
| `trigger`   | `Option<String>`                  | no       |

Setting these has no runtime effect today, but the schema is
forward-compatible — names you choose now will keep working.

## Calibration interplay

`config.toml` and `.heal/calibration.toml` are read together at every
classification. The cascade for a given metric value is:

1. **`floor_critical`** (config overrides calibration override) — if
   the value `>= floor_critical`, classify Critical immediately.
2. **`floor_ok`** — if the value `< floor_ok`, classify Ok immediately.
3. **Spread gate** — if `(p95 - p50) < (floor_critical - floor_ok) / 2`,
   the percentile classifier has no signal (everyone clustered between
   the floors). Falls to Ok.
4. **Percentile cascade** — `>= p95` → Critical, `>= p90` → High,
   `>= p75` → Medium, else Ok.

So when this skill recommends a `floor_critical` / `floor_ok` value, it
shifts the literature anchors but **does not** change the percentile
breaks. To change the breaks, recalibrate (`heal calibrate --force`)
against a different distribution — typically by editing
`exclude_paths` first so the calibration sample no longer includes
the noise.

A calibration with too few samples
(`< MIN_SAMPLES_FOR_PERCENTILES = 5`) carries `NaN` percentiles and
the cascade falls back to floor-only classification. This is normal
on tiny repos; the metric still works — it just relies on the absolute
floors.

## Strictness recipes

The `heal-config` skill picks per-strictness values from this table.
Every key not listed stays at its shipped default.

| Key                                       | Strict                                  | Default                                            | Lenient                  |
|-------------------------------------------|-----------------------------------------|----------------------------------------------------|--------------------------|
| `metrics.ccn.floor_ok`                    | `8`                                     | (literature `11` from `core::calibration`)         | `14`                     |
| `metrics.ccn.floor_critical`              | `20`                                    | (literature `25` from `core::calibration`)         | `30`                     |
| `metrics.cognitive.floor_ok`              | `5`                                     | (literature `8`)                                   | `12`                     |
| `metrics.cognitive.floor_critical`        | `35`                                    | (literature `50`)                                  | `60`                     |
| `metrics.duplication.floor_critical`      | `20`                                    | (literature `30`)                                  | `40`                     |
| `metrics.duplication.min_tokens`          | `35`                                    | `50`                                               | `75`                     |
| `metrics.change_coupling.min_coupling`    | `2`                                     | `3`                                                | `5`                      |
| `metrics.change_coupling.min_lift`        | `1.5`                                   | `2.0`                                              | `3.0`                    |
| `policy.drain.must`                       | `["critical:hotspot", "high:hotspot"]`  | `["critical:hotspot"]`                             | `["critical:hotspot"]`   |
| `policy.drain.should`                     | `["critical", "high"]`                  | `["critical", "high:hotspot"]`                     | `["critical"]`           |

A few notes on the choices:

- The Strict floors compress the literature band so even sub-Critical
  values show up; Lenient widens the same band so a mid-tier mess
  doesn't get flagged.
- `change_coupling.min_lift` is the highest-leverage strict knob.
  Going from `2.0` → `1.5` roughly doubles the surfaced pair count,
  but most newcomers above the lift floor are Symmetric (real
  coupling) rather than OneWay (release-train artefacts) — so the
  signal-to-noise is acceptable.
- The drain promotion in Strict (`high:hotspot` joins `must`) is the
  single biggest behavioural change — it forces `heal-code-patch` to
  drain hotspot-flagged High findings as if they were Critical.

### When Strict is the wrong choice

Strict assumes the codebase is dense enough that its lowered floors
sit *below* the codebase's natural distribution. When the calibration
shows the opposite — `Strict.floor_ok > calibration.p95` for `ccn` or
`cognitive` — Strict produces a Critical floodgate rather than
sharper signal. The reason is the cascade order: a value that's
≥ `floor_ok` exits the floor branch and enters the percentile
classifier; if it's also ≥ p95 (which it now is, since `floor_ok > p95`),
it lands at Critical immediately. The Medium / High band is empty,
and the drain queue is dominated by normal codebase code.

Pick Default in that case. Default's literature anchors (`floor_ok=11`
for CCN, `floor_ok=8` for Cognitive) sit comfortably above most
calibration p95 values, so the cascade has a meaningful Medium /
High zone before Critical fires.

The `heal-config` skill checks for this fit before offering Strict
and surfaces a warning in the strictness question — see SKILL.md
Phase 2.7. The check is purely advisory; the user can still pick
Strict if they want the "flag every function above CCN=8" behaviour
(useful in cryptography / safety-critical domains where CCN=8 is
genuinely the bar).

## Hand-edit hygiene

The skill preserves keys outside its recipe. If you hand-edit the
file:

- Don't introduce keys that `deny_unknown_fields` will reject. Run
  `heal status --refresh --json` once after editing to confirm the
  loader accepts the file.
- Treat `.heal/calibration.toml` as machine-owned. Override floors via
  `config.toml` (the per-metric sections) so a recalibration doesn't
  clobber them.
- Re-run `heal-config` after large structural changes (a new
  `vendor/` or `generated/` tree, a layer rewrite). Survey the
  codebase first; the previous excludes may no longer be enough.
