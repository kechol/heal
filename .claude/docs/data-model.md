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
  `change_coupling.symmetric`, `change_coupling.drift`, `hotspot`,
  `lcom`, `coverage_pct`, `skip_ratio`, plus the `[features.docs]`
  family.
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
`.heal/findings/latest.json`. Schema-versioned. **Tracked** alongside
`config.toml` and `calibration.toml` so teammates on the same commit
see identical drain queues without re-scanning.

```rust
pub const FINDINGS_RECORD_VERSION: u32 = 5;

pub struct FindingsRecord {
    pub version: u32,                // currently 5
    pub id: String,                  // FNV-1a hex of (head_sha, config_hash, worktree_clean)
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

`id` is a **deterministic** 16-hex digest, not a ULID. The same
`(head_sha, config_hash, worktree_clean)` always produces the same
id. This is what lets `latest.json` be tracked: re-running
`heal status --refresh` on a clean repo at the same commit rewrites
byte-identical content, keeping `git status` clean.

### Schema versioning

`FINDINGS_RECORD_VERSION` is currently **5**. v1 → v2 renamed
`check_id → id` and `regressed_check_id → regressed_in_record_id`.
v2 → v3 (Unreleased v0.4 cycle) bundles every new addition since
v0.3.2: the `[features.docs]` family of metric strings
(`doc_freshness`, `doc_drift`, `doc_coverage`, `doc_link_health`,
`orphan_pages`, `todo_density`), the `[features.test]` family
(`coverage_pct`, `change_coupling.drift`, `skip_ratio`), and the
new `Finding.is_test_file: bool` flag (default `false`,
`skip_serializing_if`-defaulted). v3 → v4 (same Unreleased v0.4
cycle) added the per-family hotspot metrics (`test_hotspot`,
`doc_hotspot`) and re-targeted `Finding.hotspot` to be per-family —
a `coverage_pct` Finding now takes `hotspot=true` from the
test-family index rather than the code-family one. The JSON shape
is unchanged (still a single `bool`); the meaning is not, hence
the bump. v4 → v5 (Unreleased) changed content-seed strategies:
the change-coupling family drops the raw co-change `count` from
its seed (the count drifts with every rescan, so ids churned and
`fixed.json` / `heal diff` mis-reconciled), and `ccn` /
`cognitive` / `lcom` seeds gain an occurrence ordinal so two
same-name same-span functions (or same-name class scopes) in one
file no longer collide to a single id.

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
    pub regressed_in_record_id: String,  // FindingsRecord.id (deterministic FNV-1a hex)
    pub regressed_at: DateTime<Utc>,     // Utc::now() at regression-detection time
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

## `AcceptedFinding` and `AcceptedMap` (`core::accepted`)

The team's "won't fix / acknowledged intrinsic" lane. Distinct from
`fixed.json` — accepted entries are not consumed on re-detection;
they suppress the finding's drain-queue presence indefinitely until
explicitly removed via `heal mark accept --remove` (or hand-edit).

```rust
pub struct AcceptedFinding {
    pub reason: String,                  // free-form; empty allowed
    pub file: String,                    // snapshot at accept time
    pub metric: String,                  // snapshot at accept time
    pub severity: Severity,              // snapshot at accept time
    pub hotspot: bool,                   // snapshot at accept time
    pub metric_value: Option<f64>,       // CCN / Cognitive only
    pub summary: String,                 // snapshot at accept time
    pub accepted_at: DateTime<Utc>,
    pub accepted_by: Option<String>,     // "Name <email>" from git config
}

pub type AcceptedMap = BTreeMap<String, AcceptedFinding>;
```

`AcceptedMap` is keyed by `Finding.id` and serialized as
`.heal/findings/accepted.json` (tracked, atomic-write). Schema is
`#[serde(deny_unknown_fields)]` — a schema rename requires a docs
sweep here, in `glossary.md`, and in `CHANGELOG.md`.

`Finding.accepted: bool` is **decorated at render time** by
`decorate_findings(&mut [Finding], &AcceptedMap)`; `latest.json`
keeps raw observer truth and never carries `accepted: true`. Every
renderer (`heal status`, `heal diff`, the post-commit nudge, JSON
output) folds the accepted map in just before emitting. This keeps
the observer cache cheap to write and lets policy decisions (accept
/ remove) take effect without a rescan.

`reconcile_accepted(&AcceptedMap, &[Finding]) -> Vec<AcceptedDrift>`
surfaces severity escalations only. An accepted-at-`High` finding
that now classifies as `Critical` produces an `AcceptedDrift` so
the renderer can warn. Other shapes — file deleted, finding no
longer detected, metric value moved within the same severity —
stay quiet by design (they belong on `heal mark accept --list`,
not in the live status banner). Severity is HEAL's decision
boundary; raw metric values are an implementation detail of the
classifier.

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
Config { project, git, metrics, policy, diff, features }
  ├── ProjectConfig { response_language, workspaces: Vec<WorkspaceOverlay> }
  ├── GitConfig { since_days = 90, exclude_paths }
  ├── MetricsConfig { top_n = 5, loc, churn, hotspot, change_coupling,
  │                   duplication, ccn, cognitive, lcom }
  ├── PolicyConfig { drain, rules }
  ├── DiffConfig { max_loc_threshold = 200_000 }
  └── FeaturesConfig { docs, test }
        ├── DocsConfig { enabled = false,
        │                 pairs_path = ".heal/doc_pairs.json",
        │                 scaffold_root = ".heal/docs",
        │                 standalone, doc_freshness }
        │     ├── StandaloneDocsConfig { include, exclude }
        │     └── DocFreshnessConfig { high_commits = 5,
        │                               critical_commits = 20 }
        └── TestConfig { enabled = false, test_paths, coverage }
              └── TestCoverageConfig { enabled = false, lcov_paths,
                                       post_commit_refresh = None }
```

`DuplicationConfig` adds a `docs_min_tokens = 100` field that the
Markdown duplication pass uses when `[features.docs]` is on. The
field is on `DuplicationConfig` rather than under `[features.docs]`
because the underlying observer is `Duplication`; gating logic in
`run_all` skips the Markdown pass when the feature flag is off.

`TestConfig.test_paths` defaults cover Rust (`tests/**`,
`**/*_test.rs`), JS / TS (`**/*.test.{ts,tsx,js,jsx}`,
`**/*.spec.*`, `**/__tests__/**`), Go (`**/*_test.go`), Python
(`**/test_*.py`, `**/*_test.py`), and Scala
(`**/*Test.scala`, `**/*Spec.scala`). When the list is empty the
fallback heuristic in `observer/shared/file_role.rs::is_test_path`
applies. `TestCoverageConfig.lcov_paths` defaults to `lcov.info`,
`coverage/lcov.info`, `target/llvm-cov/lcov.info`,
`coverage/lcov-report/lcov.info`; the first existing file wins.

### Schema invariants

- All structs derive `#[serde(deny_unknown_fields)]`. Typos surface as
  schema errors (`ConfigInvalid`) instead of silently dropping. **Never
  relax this** — better to require explicit migration.
- Code metrics are on by default; opt out via the top-level
  `[metrics] disabled = ["lcom", "duplication", ...]` array. Names
  are validated against the closed set in `DISABLEABLE_METRICS`
  (every code metric **except `loc`**, which is foundational).
  Per-metric sections (`[metrics.<m>]`) hold tunables only — the
  pre-v0.4 `enabled = true/false` per-section toggle is retired.
  The pin test `programmatic_default_matches_serde_default` still
  asserts `Config::default()` equals `from_toml_str("")`.
- `exclude_paths` everywhere is **`.gitignore` syntax** (since the
  `feat!(config)!: exclude_paths is now .gitignore syntax` change).
  Validated at load time; bad lines fail with `ConfigInvalid`.

### Per-metric config (defaults summarized)

| Section | Knob | Default | Notes |
|---|---|---|---|
| `[metrics]` | `disabled` | `[]` | Names from `DISABLEABLE_METRICS`; `loc` rejected. |
| `[metrics.loc]` | `inherit_git_excludes` | `true` | LOC is foundational and cannot be disabled. |
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
| `[metrics.duplication]` | `docs_min_tokens` | `100` | Markdown / RST window — only used when `[features.docs]` is on. |
| `[features.docs]` | `enabled` | `false` | Master switch for the docs family. |
| `[features.docs]` | `pairs_path` | `".heal/doc_pairs.json"` | SSoT path consumed by Layer A observers. |
| `[features.docs]` | `scaffold_root` | `".heal/docs"` | Where `/heal-doc-scaffold` writes page skeletons. HEAL itself never reads or writes this tree — consumer metadata only. |
| `[features.docs.doc_freshness]` | `high_commits` | `5` | src commits past doc → High. |
| `[features.docs.doc_freshness]` | `critical_commits` | `20` | src commits past doc → Critical. |
| `[features.docs.standalone]` | `include` | `["**/*.md", "**/*.rst"]` | Layer B globs. |
| `[features.docs.standalone]` | `exclude` | governance + generated dirs | Layer B exclusions. |
| `[features.test]` | `enabled` | `false` | Master switch for the test family. |
| `[features.test]` | `test_paths` | language conventions | gitignore-syntax globs. |
| `[features.test.coverage]` | `enabled` | `false` | Sub-feature switch for lcov ingestion. |
| `[features.test.coverage]` | `lcov_paths` | 4 conventional paths | First existing wins. |
| `[features.test.coverage]` | `post_commit_refresh` | unset | Optional shell command the post-commit hook spawns detached to refresh `lcov.info` after each commit. |
| `[diff]` | `max_loc_threshold` | `200_000` | exit 2 above this. |

### Workspace overlay

```toml
[[project.workspaces]]
path = "pkg/web"                        # project-relative, no leading /
language = "TypeScript"                 # optional override of LOC primary

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
fixed (see glossary). Used by `heal init` and by `heal-setup` skill.

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
