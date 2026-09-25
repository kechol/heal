# Glossary — canonical domain terms

This is the single source of truth for vocabulary. **Use the `Canonical`
column verbatim** in code, comments, public JSON, log lines, error
messages, user-facing docs, and skill bodies.

The term tree drifts every refactor (recent renames: `CheckRecord →
FindingsRecord`, `heal check → heal status`, `heal status → heal metrics`,
the twelve `heal-*` skills → the four `/heal:*` plugin skills). When you find drift, fix it in the same PR — see
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
| **Jev** | "the LLM", "the AI", "GPT" | TypeSafe's classifier model behind `[features.semantic]`. Returns typed probabilities, never text. Write "Jev" in prose, `jev` in commands (`heal auth jev`). |
| **semantic task** | "rule", "check", "prompt" | One kind of question HEAL asks Jev (`semantic::task::Task`). Has a stable id used as the verdict file name and the `[features.semantic.tasks.<id>]` key. |
| **concept vocabulary** | "taxonomy", "tags", "categories" | `.heal/concepts.toml`: the team's list of concepts (id + one-line responsibility) that the `concept` / `term_drift` / `doc_concept` tasks classify into. Written by `/heal-concepts-setup`. |
| **verdict** | "answer cache", "result", "judgment" | One cached Jev answer (`semantic::store::Verdict`), stored in `.heal/semantic/verdicts/<task>.jsonl` (shared tasks) or `.heal/cache/semantic/verdicts/<task>.jsonl` (the rest). |
| **on-demand task** | "manual task", "ad-hoc check" | A semantic task that runs only with `heal semantic ask --task <id>` (`Task::on_demand`): the `verify_*` checks, `name_choice`, `doc_pairs`. Its answer is `tasks[].result` in `--json`, not a Finding. |
| **shared task** | "tracked task", "team task" | A semantic task whose verdicts are team state (`Task::shared`): tracked and hashed into `config_hash`. Every task except the on-demand ones and `focus`. |
| **semantic note** | "annotation", "tag", "label" | `Finding.semantic[<name>]`, a `SemanticNote { label, p, confidence, lines, detail }` decorating an existing Finding from a cached verdict (e.g. `consequence`, `fix_ratio`). Never part of `Finding.id`. |

---

## CLI surface (live subcommands)

Use these exact strings in docs and skills. **Removed** subcommands must not
reappear in code or text.

| Canonical | Status | Was once / never use |
|---|---|---|
| `heal init` | live | Re-runnable: keeps `config.toml` and `calibration.toml` unless `--force`. `--yes` / `--no-skills` are hidden no-ops. |
| `heal doctor` | live | Read-only setup report; `/heal:setup` reads `--json`. |
| `heal hook commit` | live (post-commit only) | `heal hook stop`, `heal hook edit` are kept as **silent no-op** variants for back-compat with stale `settings.json`. New installs do **not** add them. |
| `heal status` | live (renders findings) | Was named `heal check` until v0.2 rename. **Do not** call any new feature `check`. |
| `heal metrics` | live (one-shot recompute, no cache) | Was `heal status`. The role flipped in the rename. |
| `heal diff <ref>` | live | — |
| `heal mark fix --finding-id … --commit-sha …` | live, **hidden** | Called by `/heal:refactor`, `/heal:docs`, `/heal:tests` after each commit. Do not expose in `--help`. |
| `heal mark accept --finding-id … [--reason …]` | live, **hidden** | Called by the work skills after the user approves. Do not expose in `--help`. |
| `heal mark-fixed --finding-id … --commit-sha …` | **deprecated alias** for `heal mark fix` | Hidden. Prints a stderr deprecation warning and delegates. Kept so v0.2 skill bundles keep working; the warning points at the plugin and `heal skills uninstall`. |
| `heal skills uninstall` | live | Removes skill folders older versions copied into the project. `install` / `update` / `status` are **retired** (hidden, exit 1 with the plugin pointer). |
| `heal calibrate` | live | `--reason`, `--check` were removed; do not add back. |
| `heal semantic ask` | live, opt-in (`[features.semantic]`) | The only command that sends project content over the network. |
| `heal auth jev set\|status\|clear` | live | `status` is the only other network call (key check). Keys never go under `.heal/`. |
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
| `change_coupling.drift` | (under `ChangeCoupling`) | — | `[features.test]` only. TestSrc pair whose joint count sits below the project's `change_coupling.p50`; the test isn't keeping up with its source. Severity::Medium, real Finding (not Advisory). DocSrc pairs never promote to drift. |
| `change_coupling.cross_workspace` | (Advisory tier) | — | Cross-workspace pair surfaced as Advisory. |
| `duplication` | `Duplication` | `duplication` | Type-1 (token-exact) clones over code; `[features.docs]` adds a parallel Markdown / RST pass with its own `docs_min_tokens` window. |
| `hotspot` | `Hotspot` | `hotspot` | Composite of churn × complexity, src-file keyed. **Is** an emitted metric (file-level Finding) but its severity is always `Ok` — the importance is signaled via `hotspot=true` flag on **other** Findings. |
| `lcom` | `Lcom` | `lcom` | Per-class cluster count. |
| `doc_freshness` | `DocFreshness` | `doc-freshness` | `[features.docs]` only. Per-pair "src commits since paired doc last changed." Layer A. |
| `doc_drift` | `DocDrift` | `doc-drift` | `[features.docs]` only. Type 1 dangling identifier — doc references a name no longer in the paired src AST. Layer A. |
| `doc_coverage` | `DocCoverage` | `doc-coverage` | `[features.docs]` only. Pair entries whose `doc` path is missing on disk. Layer A. |
| `doc_link_health` | `DocLinkHealth` | `doc-link-health` | `[features.docs]` only. Internal relative-path / anchor link breakage. Layer A + B. **External HTTP is out of scope** (`scope.md` R5). |
| `orphan_pages` | `OrphanPages` | `orphan-pages` | `[features.docs]` only. Layer B docs not linked from anywhere; `README.md` / `index.md` exempt as conventional entry points. |
| `todo_density` | `TodoDensity` | `todo-density` | `[features.docs]` only. Per-doc count of `TODO` / `FIXME` / `XXX` / `TBD` / `[要確認]` / `[要修正]`. Layer A + B. |
| `coverage_pct` | `Coverage` (`CoverageFeature` / `FeatureKind::CoverageReader`) | `coverage-pct` | `[features.test.coverage]` only. Per-source-file line coverage from an externally-generated lcov.info. Calibration stores **inverted** values (`100 - coverage_pct`) so the existing `value >= p95 → Critical` cascade applies; observer applies the inversion before classification. Findings emitted only for `< 100 %` files. |
| `skip_ratio` | `SkipRatio` (`SkipRatioFeature` / `FeatureKind::Observer`) | `skip-ratio` | `[features.test]` only. Per-test-file ratio of skipped tests to total tests, expressed as a percentage. Detected via tree-sitter walks of language-specific markers (Rust `#[ignore]`, Python `@pytest.mark.skip` / `@unittest.skipIf`, JS/TS `it.skip` / `xit`, Go `t.Skip()` deduped per enclosing `Test*` function, ScalaTest `ignore` / `pending`). Calibrated against `[calibration.skip_ratio]`; literature anchors > 1 % Medium / > 5 % High / > 20 % Critical via the fallback cascade. Findings emitted only for files with at least one skipped test. |
| `test_hotspot` | `TestHotspot` (`TestHotspotFeature` / `Family::Test`) | `test-hotspot` | `[features.test.coverage]` only. Per-measured-production-src-file `commits × uncov_pct` composite. LCOV-absent files are unmeasured and excluded; explicit measured 0% is a 100% gap. Severity always `Ok`; the score's job is to flip `hotspot=true` on **other** Test-family Findings (`coverage_pct`). Calibration: `HotspotCalibration` with `floor_ok = FLOOR_OK_TEST_HOTSPOT = 25.0`, configurable via `[features.test.hotspot] floor_ok`. |
| `doc_hotspot` | `DocHotspot` (`DocHotspotFeature` / `Family::Docs`) | `doc-hotspot` | `[features.docs]` only. Per-pair `paired_src_churn × debt` where `debt = src_commits_since_doc + weight_drift × dangling_idents`. Domain = paired pairs from `doc_pairs.json` (standalone docs stay covered by `orphan_pages` / `todo_density`). Severity always `Ok`; decorates Docs-family Findings via `hotspot=true` on the doc and every paired src. `weight_drift` default `1.0`; `floor_ok = FLOOR_OK_DOC_HOTSPOT = 5.0`; both configurable under `[features.docs.hotspot]`. |
| `concept_mix` | (under `Concept`) | (under `concept`) | `[features.semantic]` only. A file whose classified code spans several concepts of `.heal/concepts.toml`. |
| `concept_misplaced` | (under `Concept`) | (under `concept`) | `[features.semantic]` only. A function whose concept is another file's home concept; `fix_hint` names the move target. |
| `concept_scatter` | (under `Concept`) | (under `concept`) | `[features.semantic]` only. A concept spread over many files with no home. |
| `concept` | `Concept` | `concept` | Umbrella in CLI; selects every `concept_*` Finding. **No** Finding has `metric = "concept"`. |
| `term_drift` | (under `Naming`) | (under `naming`) | `[features.semantic]` only. Two words that name one domain thing inside a concept. |
| `name_mismatch` | (under `Naming`) | (under `naming`) | `[features.semantic]` only. A function name that contradicts or hides what the body does. |
| `naming` | `Naming` | `naming` | Umbrella in CLI; selects `term_drift` and `name_mismatch`. **No** Finding has `metric = "naming"`. |
| `test_value` | `TestValue` | `test-value` | `[features.semantic]` + `[features.test]`. A test case to delete (checks nothing real) or rewrite (brittle). |
| `mock_scope` | `MockScope` | `mock-scope` | `[features.semantic]` + `[features.test]`. A mock that replaces the subject, an internal collaborator, or a pure value. |
| `test_duplicate` | `TestDuplicate` | `test-duplicate` | `[features.semantic]` + `[features.test]`. Two cases in one file that test the same thing or could be one parameterized test. |
| `doc_structure.split` | (under `DocStructure`) | (under `doc-structure`) | `[features.semantic]` + `[features.docs]`. A page that holds more than one document. |
| `doc_structure.mixed_mode` | (under `DocStructure`) | (under `doc-structure`) | Same gate. A page that mixes documentation kinds (tutorial, how-to, reference, explanation, …). |
| `doc_structure.merge` | (under `DocStructure`) | (under `doc-structure`) | Same gate. Neighbouring short pages that read as one document. |
| `doc_placement` | `DocPlacement` | `doc-placement` | Same gate. A page that belongs in another section of the doc tree. |
| `doc_drift.semantic` | (under `DocDrift`) | (under `doc-drift`) | Same gate. Type 3 drift: a paired doc section that states what the code no longer does. |
| `doc_concept.gap` | (under `DocConcept`) | (under `doc-concept`) | Same gate plus `.heal/concepts.toml`. A sizeable concept no doc section explains. |
| `doc_concept.duplicate` | (under `DocConcept`) | (under `doc-concept`) | Same gate. Sections of two pages that repeat each other. |
| `doc_concept.conflict` | (under `DocConcept`) | (under `doc-concept`) | Same gate. Sections of two pages that contradict each other. |

Semantic Findings (every row above marked `[features.semantic]`) are
built from cached Jev verdicts by `semantic::lower::apply`, capped at
`High`.

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
| `.heal/findings/latest.json` | `heal status` writes; `heal diff` reads | **yes** | Single record. `FindingsRecord` (schema-versioned). `id` is deterministic so byte-stable across teammates. |
| `.heal/findings/fixed.json` | `heal mark fix` writes; `heal status` reconciles | **yes** | `BTreeMap<finding_id, FixedFinding>`. Bounded by outstanding claims. |
| `.heal/findings/regressed.jsonl` | `heal status` appends | **yes** | Append-only audit trail of re-detected fixes. |
| `.heal/findings/accepted.json` | `heal mark accept` writes; renderers read | **yes** | `BTreeMap<finding_id, AcceptedFinding>`. Team contract for "won't fix / intrinsic" findings. Decorates `Finding.accepted: bool` at render time. |
| `.heal/concepts.toml` | `/heal:setup` writes; semantic tasks read | **yes** | Concept vocabulary (`[[concept]] { id, description }`). Team contract; an observation input of `config_hash` while `[features.semantic]` is on. |
| `.heal/semantic/verdicts/<task>.jsonl` | `heal semantic ask` writes; `heal status` reads | **yes** | Jev verdicts of shared tasks, one per line, sorted by key. Tracked so teammates without an API key see the same Findings. |
| `.heal/cache/semantic/verdicts/<task>.jsonl` | `heal semantic ask` writes | no | Verdicts of on-demand tasks and `focus`: one person's input, never hashed. |
| `.heal/doc_pairs.json` | `/heal:setup` writes; HEAL binary reads | **yes** | `[features.docs]` SSoT. `DocPairsFile` (schema-versioned by `DOC_PAIRS_VERSION`). Maps Layer A doc paths to one or more srcs. HEAL never auto-generates it — `R3` (no auto-recalibration) extends to this file. |

| Term | Canonical | Wrong / drift |
|---|---|---|
| FindingsRecord | `FindingsRecord` (struct in `core::findings_cache`) | `CheckRecord` (renamed in commit `fea9b06`), `Snapshot`, `Report` |
| Schema version constant | `FINDINGS_RECORD_VERSION` | `CHECK_RECORD_VERSION` |
| Re-detection cross-ref | `RegressedEntry::regressed_in_record_id` | `regressed_check_id` (v1) |
| Fixed map | `FixedMap` = `BTreeMap<String, FixedFinding>` | `FixedSet` |
| Accepted map | `AcceptedMap` = `BTreeMap<String, AcceptedFinding>` | `Suppressed*`, `Ignored*`, `Allowed*` |
| Per-finding suppression | `Accepted` (state) / `accept` (verb) | `suppress`, `ignore`, `acknowledge`, `allow`, `mute` |
| Idempotency tuple | `(head_sha, config_hash, worktree_clean)` | "freshness key" — OK as prose, not as a code identifier |
| Test-file flag | `Finding.is_test_file: bool` | `is_test`, `test`, `test_path`, `under_tests` |
| Test feature config | `[features.test]`, `[features.test.coverage]` | `[features.tests]` (plural; the symmetry break with `[features.docs]` is deliberate — see CLAUDE.md). |

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

In `heal status --json` the Tier appears as `Finding.drain_tier`
(`"must"` / `"should"` / `"advisory"`) next to `Finding.drain_rank`, the
1-based position in the Finding's family queue (the rendered order,
semantic axes included). Both are render-time, like `accepted`; never
"priority", "score", or "position".

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

## Skills (the heal plugin)

Skills ship as the Claude Code plugin `heal` under `plugins/heal/`,
listed by the repository's marketplace (`.claude-plugin/marketplace.json`,
also named `heal`). Claude Code namespaces them `<plugin>:<skill>`, so
the canonical form is the colon form.

| Canonical skill | Replaces | Role |
|---|---|---|
| `/heal:setup` | `heal-setup` (was `heal-config`), `heal-concepts-setup`, `heal-doc-pair-setup`, `heal-test-reporter-setup` | Idempotent setup from `heal doctor --json`: init, recalibrate on approval, strictness, enable docs / test / semantic and build their inputs, clean up old skill folders. |
| `/heal:refactor` | `heal-code-review` (was `heal-code-check`), `heal-code-patch` (was `heal-code-fix`) | Diagnose code findings, propose (structural when two signals agree), apply approved proposals one commit each. |
| `/heal:docs` | `heal-doc-review`, `heal-doc-patch`, `heal-doc-scaffold` | Same loop for `[features.docs]` through Diátaxis; `scaffold` mode builds a doc tree. |
| `/heal:tests` | `heal-test-review`, `heal-test-patch` | Same loop for `[features.test]` through the test pyramid. |

`heal-cli` is retired as a skill; its content is the plugin's shared
`references/cli.md`.

| Canonical | Notes |
|---|---|
| heal plugin | `plugins/heal/`: manifest, SessionStart hook, shared references, four skills. Version == CLI version. |
| heal marketplace | `.claude-plugin/marketplace.json` named `heal`; one `git-subdir` entry pinned to `v<version>`. |
| work skill | `/heal:refactor`, `/heal:docs`, or `/heal:tests` — the three that share the apply loop. |
| apply loop | `plugins/heal/references/apply-loop.md`: diagnose → propose → approve → one commit per proposal → report. |
| proposal | One approved-or-not change in a work skill; may resolve several findings. Carries Change / Why / Resolves / Touches / Risk. |
| needs a decision | A proposal the user must settle individually (public rename, contested boundary, editorial or harness choice). |
| legacy skills | Skill directories older versions copied into projects; `legacy_skills::NAMES` × `ROOTS`. Removed by `heal skills uninstall`. |
| `heal-local` marketplace | **Removed.** The pre-v0.2 project-local layout (`.claude/plugins/heal/`, a `heal-local` marketplace file and settings keys), swept by `heal skills uninstall`. |
| bundled tree, metadata block, canonical bytes, drift, sidecar manifest | **Removed** with CLI skill bundling. Don't reintroduce. |

Codex CLI is not a supported target; its users may copy
`plugins/heal/skills/*` into `.agents/skills/` by hand.

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
- `/heal:setup`, `/heal:refactor`, `/heal:docs`, `/heal:tests` — colon form, never the retired `heal-*` hyphen names (except in `CHANGELOG.md` and upgrade notes).
- "the loop" not "the harness", "the run", "the cycle" (in user-facing prose).
- "workspace" not "subproject" / "package" / "module" (for the monorepo concept).
- "primary language" not "main language" / "top language".
- The product name is **HEAL** in titles and **heal** in prose / commands; never "Heal" or "heal-cli" as the brand.
