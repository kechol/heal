# Glossary — canonical domain terms

This is the single source of truth for vocabulary. **Use the `Canonical`
column verbatim** in code, comments, public JSON, log lines, error
messages, user-facing docs, and skill bodies.

The term tree drifts every refactor (recent renames: `CheckRecord →
FindingsRecord`, `heal check → heal status`, `heal status → heal metrics`,
`/heal-code-check → /heal-code-review`, `/heal-code-fix →
/heal-code-patch`). When you find drift, fix it in the same PR — see
`.claude/rules/terminology.md`.

---

## Top-level concept

| Canonical | Aliases / drift / wrong forms | Notes |
|---|---|---|
| **HEAL** | "heal-cli" (binary only), "heal-core" (retired crate) | Project name. Backronym: **H**ook-driven **E**valuation & **A**utonomous **L**oop. Always lowercase except at sentence start or in titles ("HEAL"). The single shipping crate is `heal-cli`; older `heal-core` / `heal-observer` / `heal-plugin-host` are **retired** — never reintroduce. |
| **observer** | "scanner", "analyzer", "metric runner" | A unit that produces a typed `*Report` for one metric (or composite). Lives under `crates/cli/src/observer/`. |
| **Feature** | "lowering", "classifier" | The post-observer pass that turns reports into `Vec<Finding>`. Trait `Feature` in `crates/cli/src/feature.rs`. Distinct from "metric"; one Feature can lower multiple metrics (e.g. Complexity → CCN + Cognitive). |
| **Finding** | "issue", "violation", "alert", "result" | One row in the cache. See `data-model.md`. |
| **the loop** | "drain", "harness loop" | Observe → classify → drain (review/patch). The HEAL backronym's "Loop". |

---

## CLI surface (live subcommands)

Use these exact strings in docs and skills. **Removed** subcommands must not
reappear in code or text.

| Canonical | Status | Was once / never use |
|---|---|---|
| `heal init` | live | — |
| `heal hook commit` | live (post-commit only) | `heal hook stop`, `heal hook edit` are kept as **silent no-op** variants for back-compat with stale `settings.json`. New installs do **not** add them. |
| `heal status` | live (renders findings) | Was named `heal check` until v0.2 rename. **Do not** call any new feature `check`. |
| `heal metrics` | live (one-shot recompute, no cache) | Was `heal status`. The role flipped in the rename. |
| `heal diff <ref>` | live | — |
| `heal mark fix --finding-id … --commit-sha …` | live, **hidden** | Called by `/heal-code-patch`. Do not expose in `--help`. |
| `heal mark accept --finding-id … [--reason …]` | live, **hidden** | Called by `/heal-code-review`. Do not expose in `--help`. |
| `heal mark-fixed --finding-id … --commit-sha …` | **deprecated alias** for `heal mark fix` | Hidden. Prints a stderr deprecation warning and delegates. Kept so v0.2 skill bundles keep working until `heal skills update`. |
| `heal skills install\|update\|status\|uninstall` | live | — |
| `heal calibrate` | live | `--reason`, `--check` were removed; do not add back. |
| ~~`heal checks`~~ | **removed** | Old persistent-snapshots view. |
| ~~`heal compact`~~ | **removed** | Compaction job for retired snapshots. |
| ~~`heal logs`~~ | **removed** | Log-rotation viewer. |
| ~~`heal snapshots`~~ | **removed** | Persistent metrics history. |
| ~~`heal fix`~~ group | **removed** | Was the cache TODO viewer; replaced by `heal status` + `heal mark-fixed`. |

---

## Metric names

Two surface forms, intentionally distinct (`crates/cli/src/cli.rs:118-156`):

- **CLI flag** value: kebab-case (`--metric change-coupling`).
- **JSON key** in payloads: snake_case (`payload["change_coupling"]`).

Skills can do `payload[payload.metric]` without translation because the JSON
key form matches `MetricsConfig` field names.

| Canonical metric string (Finding.metric / JSON key) | CLI kind (`MetricKind::*`) | CLI flag (kebab) | Notes |
|---|---|---|---|
| `loc` | `Loc` | `loc` | Inventory only — does not emit `Finding`. |
| `ccn` | (under `Complexity`) | (under `complexity`) | Per-function McCabe. |
| `cognitive` | (under `Complexity`) | (under `complexity`) | Per-function Sonar Cognitive. |
| `complexity` | `Complexity` | `complexity` | Umbrella in CLI; selects both `ccn` and `cognitive` Findings. **No** Finding has `metric = "complexity"`. |
| `churn` | `Churn` | `churn` | Inputs to Hotspot; does not emit `Finding`. |
| `change_coupling` | `ChangeCoupling` | `change-coupling` | One-way pair. |
| `change_coupling.symmetric` | (under `ChangeCoupling`) | (under `change-coupling`) | Both directions strong. |
| `change_coupling.expected` | (Advisory tier) | — | TestSrc / DocSrc demoted to Advisory at `Severity::Medium`. |
| `change_coupling.cross_workspace` | (Advisory tier) | — | Cross-workspace pair surfaced as Advisory. |
| `duplication` | `Duplication` | `duplication` | Type-1 (token-exact) clones. |
| `hotspot` | `Hotspot` | `hotspot` | Composite of churn × complexity. **Is** an emitted metric (file-level Finding) but its severity is always `Ok` — the importance is signaled via `hotspot=true` flag on **other** Findings. |
| `lcom` | `Lcom` | `lcom` | Per-class cluster count. |

Don't invent new submetric strings without bumping `FINDINGS_RECORD_VERSION`
(see `.claude/rules/data-model.md`).

---

## Severity ladder

Four steps. Stable JSON form: **lowercase**.

| Canonical | Variant | Drain Tier (default) | Notes |
|---|---|---|---|
| `Ok` | `Severity::Ok` | — | Default; not surfaced unless `--all`. |
| `Medium` | `Severity::Medium` | Advisory ℹ️ | Includes demoted `change_coupling.expected` / `change_coupling.cross_workspace`. |
| `High` | `Severity::High` | T1 Should 🟡 | |
| `Critical` | `Severity::Critical` | T0 Must 🎯 (when also `hotspot=true`) | |

Aggregation rule: per-file, severity is `cmp::max` over all findings on that
file ("worst-finding-wins"). Don't replace this with weighted averaging.

The label "Critical & Hotspot" is what the user removes; "Critical alone"
is High-priority but not necessarily the drain target. Don't conflate.

---

## Severity escape hatches

`floor_critical` (escalate above this raw value) and `floor_ok` (demote
below). Constants live in `core::calibration` (`FLOOR_CCN`,
`FLOOR_COGNITIVE`, `FLOOR_DUPLICATION_PCT`, `FLOOR_OK_CCN`,
`FLOOR_OK_COGNITIVE`, `FLOOR_OK_HOTSPOT`). Values and overrides are in
`data-model.md`.

---

## Cache and persistence (`.heal/`)

| Canonical path | Owner | Tracked in git? | Notes |
|---|---|---|---|
| `.heal/config.toml` | user-edited (`heal init` writes default) | **yes** | `Config` schema; `deny_unknown_fields`. |
| `.heal/calibration.toml` | `heal calibrate` writes; user can hand-edit floors | **yes** | Per-team determinism — teammates see identical findings on same commit. |
| `.heal/.gitignore` | `heal init` writes | **yes** | Empty (comment only). Reserved for future per-machine carve-outs. |
| `.heal/findings/latest.json` | `heal status` writes; `heal diff` reads | **yes** | Single record. `FindingsRecord` (schema-versioned). `id` is deterministic so byte-stable across teammates. |
| `.heal/findings/fixed.json` | `heal mark fix` writes; `heal status` reconciles | **yes** | `BTreeMap<finding_id, FixedFinding>`. Bounded by outstanding claims. |
| `.heal/findings/regressed.jsonl` | `heal status` appends | **yes** | Append-only audit trail of re-detected fixes. |
| `.heal/findings/accepted.json` | `heal mark accept` writes; renderers read | **yes** | `BTreeMap<finding_id, AcceptedFinding>`. Team contract for "won't fix / intrinsic" findings. Decorates `Finding.accepted: bool` at render time. |

| Term | Canonical | Wrong / drift |
|---|---|---|
| FindingsRecord | `FindingsRecord` (struct in `core::findings_cache`) | `CheckRecord` (renamed in commit `fea9b06`), `Snapshot`, `Report` |
| Schema version constant | `FINDINGS_RECORD_VERSION` | `CHECK_RECORD_VERSION` |
| Re-detection cross-ref | `RegressedEntry::regressed_in_record_id` | `regressed_check_id` (v1) |
| Fixed map | `FixedMap` = `BTreeMap<String, FixedFinding>` | `FixedSet` |
| Accepted map | `AcceptedMap` = `BTreeMap<String, AcceptedFinding>` | `Suppressed*`, `Ignored*`, `Allowed*` |
| Per-finding suppression | `Accepted` (state) / `accept` (verb) | `suppress`, `ignore`, `acknowledge`, `allow`, `mute` |
| Idempotency tuple | `(head_sha, config_hash, worktree_clean)` | "freshness key" — OK as prose, not as a code identifier |

**Removed concepts** — never reintroduce these names. The retired list
lives in `architecture.md` ("What does **not** exist") and as a hard
rule in `.claude/rules/terminology.md` R3.

---

## Drain policy and tiers

Drain tiers (from `[policy.drain]` in config and `core::config::DrainTier`):

| Canonical | Default rule | Renderer label |
|---|---|---|
| `DrainTier::Must` | `critical:hotspot` | T0 Must 🎯 |
| `DrainTier::Should` | `critical`, `high:hotspot` | T1 Should 🟡 |
| `DrainTier::Advisory` | everything else surfaced | Advisory ℹ️ |

`DrainSpec` syntax: `severity` or `severity:hotspot` (e.g.
`critical:hotspot`). The `:hotspot` suffix means "Required" — match only
when the Finding has `hotspot=true`.

---

## Workspaces (monorepos)

Use **workspace** consistently. Avoid "subproject", "package", "module",
"folder" for this concept.

| Canonical | Notes |
|---|---|
| `WorkspaceOverlay` | Per-workspace declaration in `[[project.workspaces]]`. |
| `Finding.workspace: Option<String>` | Tagged post-classify by longest-prefix match (`assign_workspace`). |
| `MonorepoSignal` | Detection result from `core::monorepo::detect`; presence-only, **not** enumeration. |
| primary language | `LocReport.primary` — highest-`code` non-literate language. **Markdown is not primary.** |

The list of detected manifests is fixed: `package.json` (with
`workspaces`), `pnpm-workspace.yaml`, `Cargo.toml` (with `[workspace]`),
`go.work`, `nx.json`, `turbo.json`. Don't invent custom signals — extend
the enum in `core::monorepo` instead.

---

## Skills (Claude Code)

Skills under `crates/cli/skills/`. Names are kebab-case and
prefixed `heal-`. Trigger forms in skill bodies are slash-commands.

| Canonical skill name | Slash-command form | Role |
|---|---|---|
| `heal-cli` | `/heal-cli` | CLI reference (read-only). |
| `heal-config` | `/heal-config` | Calibrate + write `.heal/config.toml`. |
| `heal-code-review` | `/heal-code-review` | Read-only architectural analysis. **Was** `heal-code-check`. |
| `heal-code-patch` | `/heal-code-patch` | Drain cache, one finding per commit. **Was** `heal-code-fix`. |

The pair `heal-code-review` ↔ `heal-code-patch` is intentional: review =
read-only, patch = mechanical write. Don't merge them.

| Canonical | Notes |
|---|---|
| metadata block | YAML `metadata:` in SKILL.md frontmatter. Carries `heal-version`, `heal-source`. |
| canonical bytes | On-disk SKILL.md with `metadata:` block stripped (`skill_assets::strip_skill_metadata`). |
| drift | Function of (canonical on-disk) vs. bundled raw bytes. Not a timestamp; not a state file. |
| bundled tree | The `crates/cli/skills/` directory embedded via `include_dir!`. |
| sidecar manifest | **Removed.** `skills-install.json` no longer exists. |
| HEAL plugin tree | **Removed.** `.claude-plugin/marketplace.json`, `.claude/plugins/heal/` are pre-v0.2 layouts swept on install. |

---

## PairClass (change-coupling pair filtering)

Internal classification used to demote noisy coupling pairs. See
`crates/cli/src/observer/change_coupling.rs`.

| Canonical | Action | Notes |
|---|---|---|
| `Lockfile` | drop | `package-lock.json`, `go.sum`, `*.lock`. |
| `Generated` | drop | `dist/`, `build/`, `target/`, `__pycache__/`, `*.min.js`, `*.snap`. |
| `Manifest` | drop | `mod.rs` ↔ sibling, `__init__.py` ↔ sibling — vertical re-export. |
| `TestSrc` | demote to Advisory | `change_coupling.expected`, `Severity::Medium`. |
| `DocSrc` | demote to Advisory | `change_coupling.expected`, `Severity::Medium`. |
| `Genuine` | keep | Drain-eligible. |

---

## Coupling direction

| Canonical | Notes |
|---|---|
| `OneWay { from, to }` | Conditional probability asymmetric. Single-arrow render. |
| `Symmetric` | Both `P(B|A)` and `P(A|B)` ≥ `symmetric_threshold` (default 0.5). Metric tag becomes `change_coupling.symmetric`. |

---

## Hashing and errors

`core::hash`: `fnv1a_64`, `fnv1a_64_chunked`, `fnv1a_hex` — the only
hashers used for persistent identity. `core::Error` / `core::Result<T>`
in `core::error`. Details in `data-model.md`.

---

## Things called by the wrong name (drift watch list)

Cross-check before merging:

- `Finding` not "issue" / "violation" / "alert".
- `FindingsRecord` not "CheckRecord" / "Snapshot" / "Report".
- `worktree_clean` not "is_clean" / "dirty=false".
- `severity_counts` not "summary" / "tally" (in JSON; `SeverityCounts::tally` is the method name, fine in prose).
- `head_sha` not "commit" / "ref".
- `config_hash` not "fingerprint".
- `regressed_in_record_id` not "regressed_check_id" / "in_record".
- `change_coupling` not "co-change" / "co-occurrence" (those are internal mechanism descriptions, fine in comments).
- `heal-code-review` and `heal-code-patch` not "heal-check" / "heal-fix" / "code-check" / "code-fix" — the prefix `heal-` is mandatory and the verbs `review`/`patch` are the canonical ones.
- "the loop" not "the harness", "the run", "the cycle" (in user-facing prose).
- "workspace" not "subproject" / "package" / "module" (for the monorepo concept).
- "primary language" not "main language" / "top language".
- The product name is **HEAL** in titles and **heal** in prose / commands; never "Heal" or "heal-cli" as the brand.
