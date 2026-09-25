# Internal architecture

Layered view of `heal-cli` (the only published crate; binary `heal`).

```
┌──────────────────────────────────────────────────────────────────────┐
│ entrypoint  src/main.rs → src/cli.rs (clap derive) → commands/*      │
├──────────────────────────────────────────────────────────────────────┤
│ commands/                                                            │
│   init      hook      status      diff      mark                     │
│   metrics/  calibrate skills                                         │
├──────────────────────────────────────────────────────────────────────┤
│ orchestrator           src/observers.rs                              │
│   run_all() → ObserverReports → build_calibration() → classify()     │
├──────────────────────────────────────────────────────────────────────┤
│ feature lowering       src/feature.rs                                │
│   FeatureRegistry::builtin().lower_all() → Vec<Finding>              │
│   each Feature: classify against Calibration, decorate hotspot flag  │
├──────────────────────────────────────────────────────────────────────┤
│ observers              src/observer/{code,docs,test,shared}/        │
│   code/  loc  complexity{ccn,cognitive}  churn  change_coupling      │
│          duplication  hotspot  lcom                                  │
│   docs/  doc_pairs  doc_freshness  doc_drift  doc_coverage           │
│          doc_link_health  orphan_pages  todo_density  markdown       │
│          (gated on cfg.features.docs.enabled)                        │
│   test/  coverage_pct  skip_ratio  lcov reader                       │
│          (gated on cfg.features.test.enabled)                        │
│   shared/ walk (gitignore + workspace) lang (tree-sitter)            │
│           git (git2 + shared history) file_role (is_test_file)       │
├──────────────────────────────────────────────────────────────────────┤
│ core                   src/core/*                                    │
│   config  calibration  finding  findings_cache  source_cache         │
│   paths  fs  hash  monorepo  term  error                             │
├──────────────────────────────────────────────────────────────────────┤
│ harness integration    src/legacy_skills.rs  src/claude_settings.rs  │
│   cleans up what older versions wrote (heal skills uninstall);       │
│   skills ship separately as the plugin in plugins/heal/              │
└──────────────────────────────────────────────────────────────────────┘
```

The crate split (`heal-core` / `heal-observer` / `heal-cli`) was inlined
into a single crate so `cargo install heal-cli` is the one supported
install path. Module shape (`crate::core::*`, `crate::observer::*`) is
preserved so call sites read the same as before. `lib.rs` is
`#[doc(hidden)]` and treated as **unstable internal API** — the public
contract is the `heal` CLI surface.

---

## End-to-end flow: `heal status`

```
heal status [--refresh]
  ↓
commands::status::run
  ↓
read_latest_if_fresh(.heal/findings/latest.json) (unless --refresh)
  ↓
fresh HEAD/clean gate + config/calibration/observation-input hash?
  ├── yes → cached raw record; skip scan and continue at accepted overlay
  └── no / --refresh → stable scan below
  ↓
stable input window begins: hash before → load current Config + existing
Calibration (normal status does not build or rewrite calibration)
  ↓
observers::run_all(project, cfg, only=None, workspace=None)
  ├── LocObserver               (always)
  ├── ComplexityObserver        (CCN + Cognitive in one pass)
  ├── ChurnObserver             (cfg-gated)
  ├── ChangeCouplingObserver    (cfg-gated; promotes TestSrc → drift on
  │                              [features.test])
  ├── DocPairsObserver          (loads .heal/doc_pairs.json;
  │                              [features.docs] only)
  ├── DuplicationObserver       (cfg-gated; +Markdown pass on
  │                              [features.docs])
  ├── HotspotObserver           (composes Churn + Complexity)
  ├── LcomObserver              (cfg-gated)
  ├── DocFreshness, DocDrift,   ([features.docs] only)
  │   DocCoverage, DocLinkHealth,
  │   OrphanPages, TodoDensity
  └── CoverageObserver,         ([features.test] / .test.coverage)
      SkipRatioObserver
  source walk uses at most 8 workers, rejoins by original path order,
  and validates reusable derived data in .heal/cache/source-v1.json
  ↓
feature::FeatureRegistry::builtin().lower_all(reports, cfg, cal)
  → Vec<Finding> with severity + hotspot flag
  ↓
hash observation inputs after scan
  ├── changed → retry full window up to 3 times, then stop without writing
  └── stable  → continue
  ↓
FindingsRecord { id (FNV-1a of head+config+clean), head_sha, config_hash,
                 worktree_clean, severity_counts, workspaces, findings }
  ↓
fs::atomic_write → .heal/findings/latest.json
  ↓
reconcile_fixed(fixed.json, regressed.jsonl, &record)
  → re-detected fixes move to regressed.jsonl, dropped from fixed.json
  ↓
read accepted.json → overlay accepted state + ephemeral re-review notices
  (latest.json remains raw observer truth)
  ↓
render or filtered JSON → spawn pager (stdout TTY && !--no-pager && !--json)
```

## End-to-end flow: `heal diff <ref>`

```
heal diff [<ref> = HEAD]
  ↓
git rev-parse <ref> → from_sha
  ↓
resolved ref == checked-out HEAD, and latest.json matches
(head_sha, observation-input config_hash, worktree_clean=true)?
  ├── yes → use cached "from" record (fast path)
  └── no, including every older ref → continue
  ↓
LOC gate: scan current worktree LOC; > [diff].max_loc_threshold
                                       (default 200_000) → exit 2
  ↓
git worktree add --detach <tmp> <from_sha>
  → WorktreeGuard (Drop tears down on ? short-circuit)
  ↓
run observers + classify against current config + calibration
  → "from" FindingsRecord (today's rules applied to historical source)
  ↓
run observers on live worktree without persisting
  → "to" FindingsRecord (always fresh)
  ↓
diff buckets: resolved, regressed, improved, new_findings, unchanged
  ↓
render
```

The "from" record applies **today's** rules to historical source. This is
deliberate — apples-to-apples drift, not "what users saw at the time".
The observation-input hash includes config/calibration plus every enabled
LCOV/doc-pair logical path, state, and content. Older refs are always
materialised so ignored inputs in the live checkout cannot masquerade as
historical observations.

## End-to-end flow: post-commit

```
git commit
  ↓
.git/hooks/post-commit (installed by heal init, marker
                         "# heal post-commit hook")
  ↓
heal hook commit
  ↓
.heal/ exists? → no → silent exit 0
  ↓
load config (silent exit if missing)
  ↓
observers::run_all → classify → write_nudge
  ├── 0 critical/high → "heal: recorded · clean"
  └── else            → "heal: recorded · X critical, Y high · heal status"
                         (+ optional second line "         · N uncovered hotspot"
                          when [features.test.coverage] is on)
```

Failures are swallowed (`heal hook commit || true`) so HEAL never blocks a
commit.

---

## Pipeline ordering

`observers::run_all` keeps report assembly in a fixed order. Its shared source
stage analyzes files with at most eight workers, reuses parsers within each
worker, and rejoins results in the original path order before aggregation.

Order is fixed and meaningful:

1. **Loc** first — scans the worktree, computes primary language. Other
   observers consume `LocReport.primary` (e.g. ChangeCoupling's PairClass
   filter is language-aware).
2. **Complexity** — the shared tree-sitter source pass produces CCN and
   Cognitive together, plus the inputs needed by Duplication and Lcom.
3. **Churn** — a shared git history collector walks the `since_days` window
   once and diffs each commit against **its first parent only** (avoids
   double-counting merge commits).
4. **ChangeCoupling** — consumes the same collected history. Its pair counts
   apply the lift filter and PairClass demotion; commits with more than
   `BULK_COMMIT_FILE_LIMIT = 50` files are excluded here, not from Churn.
5. **Duplication** — tree-sitter token streams + Rabin-Karp rolling hash
   keyed by FNV-1a 64-bit per token (kind_id + text). When
   `[features.docs]` is on, a parallel Markdown / RST pass runs with
   its own `docs_min_tokens` window (default 100).
6. **Hotspot** — pure composition: zips Churn & Complexity by file,
   `(weight_complexity × ccn_sum) × (weight_churn × commits)`. Default
   weights both 1.0. Single-axis composite — Test/Docs signals get
   their own per-family hotspots (planned), not a boost on this one.
7. **Lcom** — tree-sitter class-scope walk + union-find on field/method
   accesses.

When `[features.docs]` is enabled, six additional doc-family observers
run after the code observers (DocFreshness, DocDrift, DocCoverage,
DocLinkHealth, OrphanPages, TodoDensity); when `[features.test]` is
enabled, the test-family observers (CoveragePct, SkipRatio) run too.
Both families are no-ops when their feature flag is off.

Adding a new observer: register in `run_all`, add a Feature in
`feature.rs`, plumb config + calibration sections.

---

## Severity classification — where it lives

All severity assignment happens in `Feature::lower()` — not in
observers. Observers emit findings with `severity = Ok`; the Feature
pass classifies via `MetricCalibration::classify(value)` and decorates
with `hotspot=true` from the `HotspotIndex`.

For the 3-gate classifier and per-feature emission order see
`data-model.md` ("Calibration") and `observers.md` (per-observer
section).

---

## Workspace scoping

Workspaces are first-class. The pipeline supports them at three layers:

- **Walk-time filter** (`walk.rs::walk_supported_files_under` /
  `path_under`): dropped early so out-of-workspace files never get parsed.
- **Per-workspace calibration tables**
  (`Calibration.workspaces: BTreeMap<String, MetricCalibrations>`): each
  workspace gets its own percentile breaks, so a strict `pkg/web` and a
  legacy `pkg/legacy` calibrate independently.
- **Workspace-tagged findings** (`Finding.workspace`): assigned
  post-classify via `assign_workspace(file, workspaces)` (longest-prefix
  match). Files outside all declared workspaces have `workspace = None`.

Workspace filtering applies **early in the walk**, never post-aggregation.
For git-based observers (Churn, ChangeCoupling), `commits_considered` is
recomputed against the in-workspace universe so lift/churn totals stay
internally consistent.

---

## What does **not** exist (defensive list)

Search results that match these names indicate stale code or docs — fix
in the same PR (see `.claude/rules/terminology.md`):

- No `heal run`, `heal logs`, `heal snapshots`, `heal compact`, `heal
  fix`, `heal checks` — all removed.
- No `state.json`, `snapshots/`, `checks/`, `docs/reports/`,
  `skills-install.json`, the `heal-local` marketplace — all removed.
- No `Snapshot` type, no `CheckRecord` type — both renamed/retired.
- No `heal-core` / `heal-observer` / `heal-plugin-host` published crates
  — inlined into `heal-cli`.
- No persistent metrics history. `heal metrics` recomputes every time.
  No delta tracking. The motivation is **per-team determinism** (see
  CLAUDE.md "No persistent metrics history" section).
- No skills inside the CLI: no `include_dir!`, no extraction, no
  `.claude/skills/heal-*` in user repos. Skills ship as the `heal`
  plugin (`plugins/heal/`) from this repository's marketplace, pinned
  to the release tag (`skills-and-hooks.md`).
- No Claude Code hooks written by the CLI. The post-commit **git** hook
  comes from `heal init`; the plugin's SessionStart hook only compares
  versions. `heal hook edit` / `heal hook stop` exist as silent no-ops
  for back-compat — `heal skills uninstall` sweeps them.
- No network access outside `heal semantic ask` and `heal auth jev
  status` (`semantic::client` is the only HTTP client). Observers,
  `Feature::lower`, `heal status`, and the post-commit hook read
  verdicts from `.heal/semantic/verdicts/` and never connect.

---

## File-by-file pointers

When you change one of these, propagate to its named friends:

| If you change… | Also touch… |
|---|---|
| `core::config::Config` | `config.toml` template in `commands/init.rs`, glossary, `docs/configuration.md` |
| `core::findings_cache::FindingsRecord` | bump `FINDINGS_RECORD_VERSION`, update `read_latest` peek, document in `data-model.md` |
| `core::calibration::FLOOR_*` | `docs/metrics.md`, glossary "floors" table |
| `cli::MetricKind` | `cli::FindingMetric` (CLI filter), `MetricsConfig` field names (must match JSON keys), glossary metric table |
| any observer | `feature.rs` Feature impl, `commands/metrics/<m>.rs` section, `tests/observer_<m>.rs` |
| `claude_settings::LEGACY_HEAL_COMMANDS` | think hard — this is the back-compat sweep list, not "things to delete". Add only when actually removing a hook entry shape. |
| `plugins/heal/skills/<skill>/SKILL.md` | a CLI flag or JSON shape it uses → also `plugins/heal/references/cli.md`; the loop → `references/apply-loop.md`; validate with `claude plugin validate --strict plugins/heal/skills` |
| `workspace.package.version` | `plugins/heal/.claude-plugin/plugin.json` `version`, the marketplace entry's `version` and `ref` (`/release` does this; `tests/plugin_manifest.rs` checks) |
| `legacy_skills::NAMES` | only when retiring a skill name that shipped |
