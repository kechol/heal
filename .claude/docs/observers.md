# Observer pipeline

Reference for every observer in `crates/cli/src/observer/` plus the
orchestrator in `crates/cli/src/observers.rs`.

For severity classification see `architecture.md` (the rule lives in
`Feature::lower`, not in observers themselves) and `data-model.md`
("Calibration"). For canonical metric names see `glossary.md`.

---

## Pipeline contract (`observers::run_all`)

Sequential, single-threaded, fixed order:

```
LOC → Complexity (CCN + Cognitive) → Churn → ChangeCoupling
    → Duplication → Hotspot → LCOM
```

`run_all(project, cfg, only, workspace)`:

- `only: Option<MetricKind>` — when set, runs only the relevant observers.
  `Hotspot` triggers Churn + Complexity as dependencies.
- `workspace: Option<&Path>` — applied **early in the walk** (and as a
  `commits_considered` recompute for git-based observers). Walk-based
  observers drop out-of-workspace files; LOC walks only the subtree.

LOC always runs (no per-config gate). Every other observer is
`enabled`-gated via its `*Config`.

---

## LOC (`observer/loc.rs`)

**What:** lines of code (code/comment/blank) and language inventory. Does
**not** emit `Finding`.

**Inputs:** `tokei::Languages::get_statistics`, post-filtered by
`ExcludeMatcher` for patterns tokei can't express (globs, anchors,
negation).

**Algorithm:**

1. Run tokei on root with substring-safe excludes inlined.
2. Post-filter file list with `ExcludeMatcher::is_excluded`.
3. Re-aggregate language totals from the filtered list.
4. Primary language = highest-`code` non-literate language
   (`LanguageType::is_literate()` excludes Markdown, Org, etc.).

**Outputs:** `LocReport { primary, languages, totals, files }`.

**Calibration:** none. LOC has no severity.

**Config:**
- `metrics.loc.inherit_git_excludes` (default `true`): fold
  `git.exclude_paths` into the LOC exclude list.
- `metrics.loc.exclude_paths`: gitignore-syntax patterns scoped to LOC.
- `metrics.loc.exclude_languages`: tokei language names to drop entirely.

**Quirks:** `is_tokei_substring_safe` (`loc.rs:178`) decides whether a
pattern can be passed to tokei directly or needs post-walk re-application.

---

## Complexity (`observer/complexity/`)

**What:** per-function McCabe CCN and Sonar Cognitive Complexity. Both
metrics in a **single** tree-sitter pass.

**Inputs:** tree-sitter parse of every supported file.

### Function extraction

`functions.scm` query per language captures `@function.scope` for every
function-shaped node (declarations, methods, arrow functions, closures,
lambdas).

### CCN rule (`complexity/ccn.rs`)

```
CCN(scope) = 1 + count(decision-point captures inside scope,
                       excluding nested function bodies)
```

Decision points come from `ccn.scm` (per-language). Logical operators
`&&`, `||`, `??` count **only** when the parent is a `binary_expression`
and the operator field matches one of those three. Other binaries
(`+`, `===`, etc.) don't increment.

Nested functions are pruned via `is_inside_nested_function`
(`complexity/mod.rs:162`) — each nested function gets its own row, so
the parent doesn't inherit nested-body complexity.

### Cognitive rule (`complexity/cognitive.rs`)

Per Sonar (2017):

- **B1 increment:** each control-flow break adds +1.
- **B2 nesting:** breaks inside nesting add the current depth on top of B1.
- **B3 no bonus for `else`:** bare `else` doesn't increase depth;
  `else if` is +1 with no nesting bonus.
- **Logical chain:** +1 for the chain plus +1 for each operator-kind
  switch (`&& → ||`, etc.).

Walker tracks `depth` as it descends nesting structures. Nested functions
pruned at scope entry.

### Output

Two findings per function (one CCN + one Cognitive) when the value > 0:

- `metric`: `"ccn"` or `"cognitive"`.
- `summary`: `"CCN=<n> <name> (<lang>)"` etc.
- `seed`: `"ccn:<span>"` / `"cognitive:<span>"` — span = `end_line -
  start_line`. **Span-based, not byte-offset based**: same function
  appearing at a different line still gets the same id.

Anonymous/lambda functions are named `<anonymous@LINE>`.

### Calibration

- `cal.ccn`: percentiles + `FLOOR_CCN = 25` / `FLOOR_OK_CCN = 11`.
- `cal.cognitive`: percentiles + `FLOOR_COGNITIVE = 50` / `FLOOR_OK_COGNITIVE = 8`.

---

## Churn (`observer/churn.rs`)

**What:** per-file commit count and line change totals over `since_days`.

**Inputs:** `git2::Repository` revwalk from HEAD back `since_days`.

**Algorithm:**

1. Time-sorted revwalk. Cut at `since_cutoff = now - since_days * 86400`.
2. **Diff each commit against its first parent only** (avoid
   double-counting in merge commits). Root commits diff against an empty
   tree, reporting their full inserted size.
3. Count `'+'` / `'-'` line origins from `DiffFormat::Patch`.
4. Workspace filter: drop out-of-workspace paths; recount
   `commits_considered` to commits that touched ≥1 in-workspace file.

**Outputs:** `ChurnReport { files, totals, since_days }`. Sorted by
commit count desc.

**Findings:** none directly. Consumed by Hotspot.

**Config:**
- `metrics.churn.enabled` (default `true`).
- `git.since_days` (default `90`). Global, not per-metric.
- `metrics.churn.exclude_paths`: gitignore-syntax.

**Quirks:** non-git repos return an empty report (silent). Bulk commits
> 50 files are still counted in churn (they're skipped only by
ChangeCoupling).

---

## Change Coupling (`observer/change_coupling.rs`)

**What:** which file pairs change together, with direction (one-way vs.
symmetric) and noise filtering.

**Inputs:** same revwalk pattern as Churn.

**Algorithm:**

1. Per-commit changeset extraction. Apply workspace + exclude filters.
2. **Bulk-commit cap:** skip commits with >`BULK_COMMIT_FILE_LIMIT = 50`
   files. Prevents lockfile bumps and mass-renames from dominating
   quadratic pair counts.
3. For each pair `(a, b)` in a changeset (canonical: `a < b`
   lexicographically), increment `pair_counts[(a, b)]`. Bump
   `file_commits[file]` for every file in the changeset.
4. **Lift filter:** `lift = pair_count × commits_considered / (count_a ×
   count_b)`. Keep pairs with `pair_count ≥ min_coupling` (default 3)
   AND `lift ≥ min_lift` (default 2.0). 1.0 = chance baseline; 2.0 is
   the conventional "interesting" threshold.
5. **Direction:** compute `P(B|A) = pair_count / count_a` and
   `P(A|B) = pair_count / count_b`. If both ≥ `symmetric_threshold`
   (default 0.5) → `Symmetric`; otherwise `OneWay { from = higher_P, to
   = lower_P }`.
6. **PairClass demotion** (post-scan, language-aware against
   `LocReport.primary`):

| PairClass | Action |
|---|---|
| `Lockfile` (`package-lock.json`, `go.sum`, `*.lock`) | drop |
| `Generated` (`dist/`, `build/`, `target/`, `__pycache__/`, `*.min.js`, `*.snap`) | drop |
| `Manifest` (`mod.rs ↔` sibling, `__init__.py ↔` sibling) | drop |
| `TestSrc` (test ↔ source) | demote → `change_coupling.expected`, `Severity::Medium`, Advisory |
| `DocSrc` (doc ↔ source) | demote → `change_coupling.expected`, `Severity::Medium`, Advisory |
| `Genuine` | keep, drain-eligible |

7. **Cross-workspace** (declared workspaces, both endpoints differ):
   retag as `change_coupling.cross_workspace`, route to Advisory.
   Configurable: `[metrics.change_coupling] cross_workspace = "surface"`
   (default) or `"hide"`.

**Outputs:**
- `metric`: `"change_coupling"` (one-way) or `"change_coupling.symmetric"`.
- After `Feature::lower` demotion: `"change_coupling.expected"` /
  `"change_coupling.cross_workspace"`.
- `location.file = a`, `location.symbol = b` (so `a→b` and `a→c` have
  distinct ids). Secondary `Location` for `b` in `locations`.

**Calibration:** percentile breaks on co-change counts. No hard floor.

**Quirks:** lift `INFINITY` for empty universes (degenerate; `min_coupling`
catches it).

---

## Duplication (`observer/duplication.rs`)

**What:** type-1 (token-exact) clones via Rabin-Karp rolling hash over
FNV-1a per-token identities.

**Inputs:** tree-sitter parse → leaf-token stream per file.

**Algorithm:**

1. Pre-order walk. Skip non-leaves, extras (comments/whitespace),
   errors, missing nodes, whitespace-only text.
2. Per-token hash = FNV-1a 64-bit over `(kind_id_le_bytes, text_bytes)`.
   Constants: `FNV_OFFSET = 0xcbf2_9ce4_8422_2325`, `HASH_BASE
   = 0x100_0000_01b3`. Wrapping arithmetic.
3. Rolling window hash of size `min_tokens` (default 50). Standard
   Rabin-Karp recurrence:
   ```
   h[0]   = sum(token_hashes[0..window])
   h[k+1] = (h[k] - oldest * base^(window-1)) * base + tokens[k+window]
   ```
4. Bucket windows by hash. For each bucket of ≥2 entries, verify by
   per-token hash slice equality (collision-proof for typical lengths).
5. Greedy forward extension: extend a matched window as long as **every**
   site agrees on the next token. Emit one maximal block per seed.
6. File-level rollup: `duplicate_pct = duplicate_tokens / total_tokens *
   100`. Every scanned file appears in the summary, even with 0%, so
   the calibration sample is the full population.

**Outputs:**
- One Finding per duplicate block.
- `metric`: `"duplication"`.
- `location` = canonical-sorted (path, start_line) primary site.
- `locations` = remaining sites.
- `summary`: `"<token_count> tokens duplicated across <N> sites"`.
- `seed`: `"dup:<token_count>:<path>:<line>;..."` over all sites
  (stable id).

**Calibration:** percentiles on per-file `duplicate_pct`.
`FLOOR_DUPLICATION_PCT = 30.0` overrideable via `[metrics.duplication]
floor_critical`.

**Quirks:** type-2/3 (parameterized / near-duplicate) clones are
**out-of-scope**. Identifiers participate in the hash, so `function foo`
≠ `function bar`. Single-threaded per file.

---

## Hotspot (`observer/hotspot.rs`)

**What:** composite of churn × complexity. The "where to refactor first"
signal.

**Inputs:** pre-computed `ChurnReport` and `ComplexityReport`. Pure
composition, no FS/git access.

**Algorithm:**

1. Zip by file path. For each file in **both** reports:
   - `ccn_sum = sum(function.ccn)`.
   - `commits = churn_file.commits`.
   - `score = (weight_complexity × ccn_sum) × (weight_churn × commits)`.
2. Files appearing in only one report (newly added with zero churn,
   etc.) get score 0 and are filtered out.

**Outputs:**
- One Finding per file with non-zero score.
- `metric`: `"hotspot"`.
- `severity`: **always `Ok`**. The point of Hotspot is the
  `hotspot=true` decoration on **other** findings. Don't make it
  Critical itself.
- `summary`: `"hotspot score=<v> (ccn_sum=<n>, churn=<m>)"`.

**Calibration:** `HotspotCalibration` = percentiles on raw scores +
`floor_ok = FLOOR_OK_HOTSPOT = 22.0` (= `2 × FLOOR_OK_CCN`).

`HotspotIndex` is built once per run. A file is a hotspot iff
`score ≥ p90` AND `score ≥ floor_ok` (when set). Used by every other
Feature to decorate findings on hotspot files.

**Config:**
- `metrics.hotspot.weight_churn` (default `1.0`).
- `metrics.hotspot.weight_complexity` (default `1.0`).
- `metrics.hotspot.floor_ok` overrides the literature default.

**Why multiplicative not additive:** a file with high churn but low
complexity (or vice versa) gets a modest score. A file with both gets
a large score. This matches the Tornhill "true risk = volatility ×
complexity" framing.

---

## LCOM (`observer/lcom.rs`)

**What:** per-class Lack of Cohesion of Methods. A class with
`cluster_count ≥ 2` is mechanically separable.

**Inputs:** tree-sitter `lcom.scm` query → class scopes.

**Supported languages:** TypeScript, Tsx, JavaScript, Jsx, Python, Rust.
Go and Scala are **no-ops** (`is_method_kind` returns false / class
story too rich for tree-sitter approx — LSP backend is reserved for
v0.5+).

**Algorithm:**

1. Class extraction:
   - TS/JS: `class_declaration`.
   - Rust: `impl_item` (both inherent and trait impls treated the same).
   - Python: `class_definition`.
2. Per method, collect self-references:
   - TS/JS: `member_expression` with `object = this`.
   - Rust: `field_expression` with `value = self`.
   - Python: `attribute` with `object = self`.
3. Build `field_to_methods[field] = {method indices touching it}` and
   `method_calls[method] = {indices it calls}`.
4. **Union-Find:** initialize each method as singleton; union all methods
   sharing a field; union caller/callee pairs. Cluster count = roots.
5. Sort clusters by size desc for determinism.

**Outputs:**
- One Finding per class with `cluster_count ≥ min_cluster_count`.
- `metric`: `"lcom"`.
- `summary`: `"LCOM=<count> clusters across <m> methods in <Class> (<lang>)"`.
- `seed`: `"lcom:<cluster_count>:<method_count>"`.

**Calibration:** percentiles on per-class `cluster_count`. No hard floor.

**Bias:** syntactic (no type resolution). Inherited fields invisible;
dynamic property access invisible; helper functions outside the class
look unrelated. **Biased toward over-reporting** — be conservative when
escalating.

---

## Shared infrastructure

### `observer/walk.rs`

- `ExcludeMatcher::compile(lines)` → `ignore::Gitignore` wrapper.
  Handles full gitignore DSL (glob, dir-only `foo/`, anchored `/foo`,
  negation `!keep`, comments). Walks ancestors so `vendor/` excludes
  nested files.
- `walk_supported_files_under(root, lang, include_under, excludes)` —
  uses `ignore::WalkBuilder` (same crate as tokei). Respects
  `.gitignore`, skips `.git/`, hidden by default. Yields only paths
  with supported extensions.
- `path_under(path, workspace)` — segment-wise check (so `pkg/web` does
  **not** match `pkg/webapp/foo.ts`). Workspaces are early-filter, never
  post-aggregate.
- `since_cutoff(since_days)` → Unix seconds threshold.

### `observer/lang.rs`

- Language registry: TypeScript, Tsx, JavaScript, Jsx, Python, Go, Scala,
  Rust. Cargo features: `lang-ts`, `lang-js`, `lang-py`, `lang-go`,
  `lang-scala`, `lang-rust` (at least one required at compile time).
- Extension dispatch: `Language::from_path`.
- Tree-sitter queries are **embedded** via `include_str!` from
  `crates/cli/queries/<lang>/<type>.scm`. Compiled once per (lang,
  query-type) via `OnceLock`. Capture indices resolved at compile time.
- Query types: `functions.scm` (CCN/Cognitive scopes), `ccn.scm`,
  `cognitive.scm`, `lcom.scm`.

### `observer/git.rs`

- Pure `git2` library — no shelling out.
- `head_sha`, `resolve_ref`, `worktree_clean` (matches `git status`
  semantics via `StatusOptions`), `head_commit_info`.

---

## Adding a new observer (checklist)

1. Add `crate::observer::<m>` module with a `*Observer`, a `*Report`, a
   `*Config` (in `core::config` with `Toggle` impl), and an
   `IntoFindings` impl on the report.
2. Wire into `observers::run_all` in dependency order.
3. Add a Feature in `crate::feature::FeatureRegistry::builtin()` with
   correct emission ordering.
4. Add calibration plumbing: a `MetricCalibrations` field if not present,
   a `from_distribution` call in `build_calibration`, classify in
   `Feature::lower`.
5. Register a metric kind in `cli::MetricKind` + `MetricKind::json_key`
   + `cli::FindingMetric` if it needs CLI filtering.
6. Add a section in `commands/metrics/<m>.rs` and register in
   `commands/metrics/mod.rs::all_sections()`.
7. Tests in `crates/cli/tests/observer_<m>.rs`.
8. Update `glossary.md` (metric strings table) and `data-model.md`
   (per-metric defaults table) in the same PR.
