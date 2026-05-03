# Data model

Cross-reference for every persistent shape HEAL produces or reads.

All canonical type names — see `glossary.md` first when in doubt.

---

## `Finding` (`core::finding`)

The cross-observer record. One row in the cache.

```rust
pub struct Finding {
    pub id: String,                  // <metric>:<file>:<symbol-or-*>:<16-hex>
    pub metric: String,              // see glossary "metric strings" table
    pub severity: Severity,          // post-classify
    pub hotspot: bool,               // post-classify decoration
    pub workspace: Option<String>,   // post-classify (longest-prefix)
    pub location: Location,          // canonical site
    pub locations: Vec<Location>,    // multi-site extras (duplication, coupling)
    pub summary: String,
    pub fix_hint: Option<String>,
}

pub struct Location {
    pub file: PathBuf,
    pub line: Option<u32>,
    pub symbol: Option<String>,
}
```

### Finding.id format (deterministic)

```
<metric>:<file>:<symbol-or-*>:<16-hex-fnv1a>
```

- `metric`: e.g. `ccn`, `cognitive`, `duplication`, `change_coupling`,
  `change_coupling.symmetric`, `hotspot`, `lcom`.
- `file`: project-relative, forward-slash separated.
- `symbol`: function/class/path-of-pair-partner; literal `*` if absent.
- The 16-hex digest is FNV-1a 64-bit over `[metric, file, symbol,
  content_seed]` chunks (separator `0xff` — see `core::hash`). The
  observer supplies `content_seed`. For position-stable identity,
  observers seed on **structural** signals (e.g. function-span length,
  duplicate-block site signature) — not on raw byte spans.

The id is **the contract**. An unfixed finding **must** reappear under
the same id on the next run (otherwise `fixed.json` reconciliation
breaks). When you change an observer's seed strategy, expect to bump
`FINDINGS_RECORD_VERSION` (see "Schema versioning" below).

### `IntoFindings` trait

```rust
pub trait IntoFindings {
    fn into_findings(&self) -> Vec<Finding>;
}
```

Each typed report (`ChurnReport`, `ComplexityReport`, …) implements this
to lower itself into findings. `&self` so the caller keeps the report
for separate rendering paths (e.g. `heal metrics`).

---

## `FindingsRecord` (`core::findings_cache`)

The full result of one `heal status` run. Written to
`.heal/findings/latest.json`. Schema-versioned.

```rust
pub const FINDINGS_RECORD_VERSION: u32 = 2;

pub struct FindingsRecord {
    pub version: u32,                // currently 2
    pub id: String,                  // ULID (Crockford-base32, ms-prefix)
    pub started_at: DateTime<Utc>,
    pub head_sha: Option<String>,    // None outside git or HEAD unborn
    pub worktree_clean: bool,
    pub config_hash: String,         // FNV-1a hex of config + calibration
    pub severity_counts: SeverityCounts,
    pub workspaces: Vec<WorkspaceSummary>,
    pub findings: Vec<Finding>,
}

pub struct WorkspaceSummary {
    pub path: String,
    pub severity_counts: SeverityCounts,
}
```

### Schema versioning

`FINDINGS_RECORD_VERSION` is currently **2**. v1 → v2 renamed
`check_id → id` and `regressed_check_id → regressed_in_record_id`.

`read_latest` peeks at the version field first and returns `Ok(None)`
on **any mismatch** — the next run silently rewrites under the new
schema. There is no migration path: bumping the version is the
prescribed escape hatch when a field rename is unavoidable.

When you bump the version:

1. Update `FINDINGS_RECORD_VERSION`.
2. Update the `read_latest` peek logic only if you need to handle
   anything other than "version mismatch → drop and rebuild".
3. Update glossary, this doc, and `CHANGELOG.md` "Unreleased" with the
   migration note. User-facing CLI surface usually doesn't change.

### Idempotency tuple

`(head_sha, config_hash, worktree_clean)` is the freshness key.

```rust
pub fn is_fresh_against(
    &self,
    head_sha: Option<&str>,
    config_hash: &str,
    worktree_clean: bool,
) -> bool {
    if !self.worktree_clean || !worktree_clean { return false; }
    self.head_sha.as_deref() == head_sha && self.config_hash == config_hash
}
```

**Dirty worktrees are never fresh.** Reading or writing this rule away
breaks the contract — the recorded numbers wouldn't reflect on-disk
source.

`config_hash` covers both `config.toml` and `calibration.toml`. A
`heal calibrate --force` shifts the hash, invalidating the cache.

---

## `FixedFinding` and `RegressedEntry`

```rust
pub struct FixedFinding {
    pub finding_id: String,
    pub commit_sha: String,
    pub fixed_at: DateTime<Utc>,
}

pub struct RegressedEntry {
    pub finding_id: String,
    pub previous_commit_sha: String,
    pub previous_fixed_at: DateTime<Utc>,
    pub regressed_in_record_id: String,  // FindingsRecord.id (ULID)
    pub regressed_at: DateTime<Utc>,
}
```

`FixedMap = BTreeMap<String, FixedFinding>` — keyed by `finding_id`.
Bounded by outstanding fix claims; never appended.

`regressed.jsonl` is **the** append-only audit trail. Format: one
`RegressedEntry` JSON per line.

`reconcile_fixed`: on every fresh `FindingsRecord`, walk all findings;
if `finding_id ∈ fixed.json`, move to `regressed.jsonl` (atomic rewrite
of `fixed.json`, append to jsonl). Don't suppress this — the warning is
the whole point of tracking fixes separately.

---

## `Severity` and `SeverityCounts` (`core::severity`)

```rust
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default] Ok,
    Medium,
    High,
    Critical,
}
```

`Ord` is derived on the variant order, so `Ok < Medium < High <
Critical`. `cmp::max` aggregation is the per-file rule.

```rust
pub struct SeverityCounts { critical: u32, high: u32, medium: u32, ok: u32 }
```

---

## `Config` (`core::config`)

Tree:

```
Config { project, git, metrics, policy, diff }
  ├── ProjectConfig { response_language, workspaces: Vec<WorkspaceOverlay> }
  ├── GitConfig { since_days = 90, exclude_paths }
  ├── MetricsConfig { top_n = 5, loc, churn, hotspot, change_coupling,
  │                   duplication, ccn, cognitive, lcom }
  ├── PolicyConfig { drain, rules }
  └── DiffConfig { max_loc_threshold = 200_000 }
```

### Schema invariants

- All structs derive `#[serde(deny_unknown_fields)]`. Typos surface as
  schema errors (`ConfigInvalid`) instead of silently dropping. **Never
  relax this** — better to require explicit migration.
- Per-metric `*Config` uses the `Toggle` trait + `default_enabled` glue
  (`config.rs:307`) so a missing `[metrics.<m>]` section deserializes
  with `enabled = true`. `Default` impls produce the **same** struct,
  pinned by test `programmatic_default_matches_serde_default`.
- `exclude_paths` everywhere is **`.gitignore` syntax** (since the
  `feat!(config)!: exclude_paths is now .gitignore syntax` change).
  Validated at load time; bad lines fail with `ConfigInvalid`.

### Per-metric config (defaults summarised)

| Section | Knob | Default | Notes |
|---|---|---|---|
| `[metrics.loc]` | `inherit_git_excludes` | `true` | LOC has no enabled flag — foundational. |
| `[metrics.churn]` | `enabled` | `true` | |
| `[metrics.hotspot]` | `weight_churn` | `1.0` | |
| `[metrics.hotspot]` | `weight_complexity` | `1.0` | |
| `[metrics.hotspot]` | `floor_ok` | `FLOOR_OK_HOTSPOT = 22.0` | |
| `[metrics.change_coupling]` | `min_coupling` | `3` | |
| `[metrics.change_coupling]` | `min_lift` | `2.0` | |
| `[metrics.change_coupling]` | `symmetric_threshold` | `0.5` | |
| `[metrics.change_coupling]` | `cross_workspace` | `Surface` | `Surface` ⇒ Advisory tag, `Hide` ⇒ drop. |
| `[metrics.duplication]` | `min_tokens` | `50` | |
| `[metrics.duplication]` | `floor_critical` | (override `FLOOR_DUPLICATION_PCT = 30.0`) | |
| `[metrics.ccn]` | `floor_critical` | (override `FLOOR_CCN = 25.0`) | |
| `[metrics.ccn]` | `floor_ok` | (override `FLOOR_OK_CCN = 11.0`) | |
| `[metrics.cognitive]` | `floor_critical` | (override `FLOOR_COGNITIVE = 50.0`) | |
| `[metrics.cognitive]` | `floor_ok` | (override `FLOOR_OK_COGNITIVE = 8.0`) | |
| `[metrics.lcom]` | `backend` | `tree-sitter-approx` | `lsp` is reserved for v0.5+. |
| `[metrics.lcom]` | `min_cluster_count` | `2` | |
| `[diff]` | `max_loc_threshold` | `200_000` | exit 2 above this. |

### Workspace overlay

```toml
[[project.workspaces]]
path = "pkg/web"                        # project-relative, no leading /
primary_language = "TypeScript"         # optional override

[project.workspaces.metrics.ccn]
floor_critical = 40
floor_ok = 18
```

Override layer order: built-in floor → `[metrics.<m>]` global override →
`[project.workspaces.metrics.<m>]` workspace override (workspace wins).

Scan-time tunables (`since_days`, `min_coupling`, `min_lift`,
`symmetric_threshold`, `min_tokens`) **remain global** — workspace
overlays handle severity floors only.

### Drain policy

```rust
pub struct PolicyDrainConfig {
    pub must: Vec<DrainSpec>,    // T0 default: ["critical:hotspot"]
    pub should: Vec<DrainSpec>,  // T1 default: ["critical", "high:hotspot"]
    pub metrics: BTreeMap<String, PolicyDrainMetricOverride>,
}

pub struct DrainSpec { severity: Severity, hotspot: HotspotMatch }
pub enum HotspotMatch { Any, Required }     // Required ⇔ ":hotspot" suffix
pub enum DrainTier { Must, Should, Advisory }
```

Spec syntax in TOML strings: `severity` or `severity:hotspot` (e.g.
`critical`, `high:hotspot`). Anything not matching `must` or `should`
falls into `Advisory`.

---

## `Calibration` (`core::calibration`)

```rust
pub struct Calibration {
    pub meta: CalibrationMeta,
    pub calibration: MetricCalibrations,                // global / fallback
    pub workspaces: BTreeMap<String, MetricCalibrations>,
}

pub struct MetricCalibration {
    pub p50: f64, pub p75: f64, pub p90: f64, pub p95: f64,
    pub floor_critical: Option<f64>,
    pub floor_ok: Option<f64>,
}

pub struct HotspotCalibration {
    pub p50: f64, pub p75: f64, pub p90: f64, pub p95: f64,
    pub floor_ok: Option<f64>,                          // FLOOR_OK_HOTSPOT
}
```

`MetricCalibration::classify(value: f64) → Severity`:

```
1. floor_critical present and value ≥ floor_critical  → Critical
2. floor_ok present and value < floor_ok              → Ok
3. spread_gate fires                                  → Ok
4. value ≥ p95                                        → Critical
5. value ≥ p90                                        → High
6. value ≥ p75                                        → Medium
7. otherwise                                          → Ok
```

`from_distribution(values, floors)` builds the table from observed data.
With < `MIN_SAMPLES_FOR_PERCENTILES` (5), percentiles are `NaN` and `≥`
comparisons against `NaN` are always `false` → cascade falls through to
`Ok`.

`HotspotCalibration::flag(score) → bool`: true iff `score ≥ p90` **and**
`score ≥ floor_ok` (when set). Sets the `hotspot=true` decoration on
findings whose file is in the top decile.

---

## `MonorepoSignal` (`core::monorepo`)

```rust
pub struct MonorepoSignal { manifest: String, kind: String }
```

`detect(project_root) → Vec<MonorepoSignal>`. Presence-only — no
enumeration of workspace members. The list of detected manifests is
fixed (see glossary). Used by `heal init` and by `heal-config` skill.

---

## `core::Error`

```rust
pub enum Error {
    ConfigMissing(PathBuf),
    ConfigParse  { path: PathBuf, source: toml::de::Error },
    ConfigInvalid{ path: PathBuf, message: String },
    CacheParse   { path: PathBuf, source: serde_json::Error },
    Io           { path: PathBuf, source: std::io::Error },
}
pub type Result<T> = std::result::Result<T, Error>;
```

Every variant carries `path` for actionable messages. Don't add path-less
variants.

---

## Atomic write contract

Every persistent state file goes through `core::fs::atomic_write`:

```
write to <path>.tmp → fsync → rename to <path>
```

Files written this way:
- `.heal/config.toml`
- `.heal/calibration.toml`
- `.heal/findings/latest.json`
- `.heal/findings/fixed.json`
- `.claude/settings.json` (when modified)
- All extracted skill files

`regressed.jsonl` is append-only; treated separately.

---

## Hashing

`core::hash`:

```rust
pub fn fnv1a_64(bytes: &[u8]) -> u64;
pub fn fnv1a_64_chunked(chunks: &[&[u8]]) -> u64;     // separator 0xff
pub fn fnv1a_hex(h: u64) -> String;                    // 16 lowercase hex
```

Constants:

```rust
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME:  u64 = 0x100_0000_01b3;
```

Used for: `Finding.id`, `FindingsRecord.config_hash`, the duplication
observer's per-token identity. **Never** swap to
`std::hash::DefaultHasher` for any persistent value — its algorithm is
unstable across Rust toolchain versions, which would invalidate every
recorded id after a `rustc` upgrade.

---

## What's **not** in the data model (sanity checks)

- No persistent metrics history. Don't add a `snapshots/` directory or
  any per-run jsonl growth path.
- No `state.json`. The cache is `findings/latest.json`; that's it.
- No sidecar manifest for skills. Drift detection is a function of
  on-disk vs. bundled bytes; metadata lives in SKILL.md frontmatter.
- No "delta vs. previous run" field anywhere. `heal diff` computes drift
  on demand from two `FindingsRecord`s.
