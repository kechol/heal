# Internal architecture

Layered view of `heal-cli` (the only published crate; binary `heal`).

```
┌──────────────────────────────────────────────────────────────────────┐
│ entrypoint  src/main.rs → src/cli.rs (clap derive) → commands/*      │
├──────────────────────────────────────────────────────────────────────┤
│ commands/                                                            │
│   init      hook      status      diff      mark_fixed               │
│   metrics/  calibrate skills                                         │
├──────────────────────────────────────────────────────────────────────┤
│ orchestrator           src/observers.rs                              │
│   run_all() → ObserverReports → build_calibration() → classify()     │
├──────────────────────────────────────────────────────────────────────┤
│ feature lowering       src/feature.rs                                │
│   FeatureRegistry::builtin().lower_all() → Vec<Finding>              │
│   each Feature: classify against Calibration, decorate hotspot flag  │
├──────────────────────────────────────────────────────────────────────┤
│ observers              src/observer/*                                │
│   loc  complexity{ccn,cognitive}  churn  change_coupling             │
│   duplication  hotspot  lcom                                         │
│   shared infra: walk.rs (gitignore + workspace) lang.rs (tree-sitter)│
│                 git.rs (git2)                                        │
├──────────────────────────────────────────────────────────────────────┤
│ core                   src/core/*                                    │
│   config  calibration  finding  findings_cache  severity             │
│   paths  fs  hash  monorepo  term  error                             │
├──────────────────────────────────────────────────────────────────────┤
│ harness integration    src/claude_settings.rs  src/skill_assets.rs   │
│   reads/writes .claude/settings.json (sweep-only, no new hooks)      │
│   embeds plugins/heal/skills/ via include_dir!                       │
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
read .heal/findings/latest.json (if not --refresh)
  ↓
is_fresh_against(head_sha, config_hash, worktree_clean)?
  ├── yes → render cached record (fast path)
  └── no  → continue
  ↓
observers::run_all(project, cfg, only=None, workspace=None)
  ├── LocObserver         (always)
  ├── ComplexityObserver  (CCN + Cognitive in one pass)
  ├── ChurnObserver       (cfg-gated)
  ├── ChangeCouplingObserver (cfg-gated)
  ├── DuplicationObserver (cfg-gated)
  ├── HotspotObserver     (composes Churn + Complexity)
  └── LcomObserver        (cfg-gated)
  ↓
observers::build_calibration(reports, config)
  → MetricCalibration per metric (global + per-workspace)
  ↓
feature::FeatureRegistry::builtin().lower_all(reports, cfg, cal)
  → Vec<Finding> with severity + hotspot flag
  ↓
FindingsRecord { id (FNV-1a of head+config+clean), head_sha, config_hash,
                 worktree_clean, severity_counts, workspaces, findings }
  ↓
fs::atomic_write → .heal/findings/latest.json
  ↓
reconcile_fixed(fixed.json, regressed.jsonl, &record)
  → re-detected fixes move to regressed.jsonl, dropped from fixed.json
  ↓
render → spawn pager (stdout TTY && !--no-pager && !--json)
```

## End-to-end flow: `heal diff <ref>`

```
heal diff [<ref> = HEAD]
  ↓
git rev-parse <ref> → from_sha
  ↓
read latest.json: head_sha == from_sha?
  ├── yes → use cached "from" record (fast path)
  └── no  → continue
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
read latest.json (or run observers on live worktree if --refresh)
  → "to" FindingsRecord
  ↓
diff buckets: resolved, regressed, improved, new_findings, unchanged
  ↓
render
```

The "from" record applies **today's** rules to historical source. This is
deliberate — apples-to-apples drift, not "what users saw at the time".

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
```

Failures are swallowed (`heal hook commit || true`) so HEAL never blocks a
commit.

---

## Pipeline ordering

`observers::run_all` is **sequential**, not parallel. The bottleneck is
tree-sitter parsing inside Complexity; no rayon-style fan-out today.

Order is fixed and meaningful:

1. **Loc** first — scans the worktree, computes primary language. Other
   observers consume `LocReport.primary` (e.g. ChangeCoupling's PairClass
   filter is language-aware).
2. **Complexity** — single tree-sitter pass per file produces both CCN
   and Cognitive.
3. **Churn** — git revwalk over `since_days` window. Diffs each commit
   against **its first parent only** (avoids double-counting merge
   commits). Cap: skip commits with > `BULK_COMMIT_FILE_LIMIT = 50` files
   for ChangeCoupling (lockfile bumps, mass-renames).
4. **ChangeCoupling** — same revwalk pattern. Pair counts → lift filter
   → PairClass demotion.
5. **Duplication** — tree-sitter token streams + Rabin-Karp rolling hash
   keyed by FNV-1a 64-bit per token (kind_id + text).
6. **Hotspot** — pure composition: zips Churn & Complexity by file,
   `(weight_complexity × ccn_sum) × (weight_churn × commits)`. Default
   weights both 1.0.
7. **Lcom** — tree-sitter class-scope walk + union-find on field/method
   accesses.

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
  `skills-install.json`, `marketplace.json` (in current `.claude-plugin/`
  position) — all removed.
- No `Snapshot` type, no `CheckRecord` type — both renamed/retired.
- No `heal-core` / `heal-observer` / `heal-plugin-host` published crates
  — inlined into `heal-cli`.
- No persistent metrics history. `heal metrics` recomputes every time.
  No delta tracking. The motivation is **per-team determinism** (see
  CLAUDE.md "No persistent metrics history" section).
- No marketplace, no plugin distribution, no `.claude/plugins/heal/`
  layout. Skills are extracted directly to `.claude/skills/<name>/` from
  the embedded tree.
- No Claude Code hooks registered by HEAL anymore. Only the post-commit
  **git** hook. `heal hook edit` / `heal hook stop` exist as silent
  no-ops for back-compat — `heal skills install` actively sweeps them.

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
| `plugins/heal/skills/<skill>/SKILL.md` | the `metadata:` block is **rewritten on extract** by `skill_assets`; do not hand-author it in source. Edit body, version is auto-injected. |
