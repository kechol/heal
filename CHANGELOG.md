# Changelog

## Unreleased

### `heal mark` group: generalises `mark-fixed`, adds `mark accept`

`heal mark-fixed` is replaced by `heal mark fix` (and the new sibling
`heal mark accept`). The legacy `heal mark-fixed` invocation still
works — it prints a one-line stderr deprecation warning and delegates
to `heal mark fix` so older `/heal-code-patch` skill bundles keep
running. Refresh the bundle with `heal skills update` to switch to
the new form.

`heal mark accept --finding-id <ID> [--reason <TEXT>]` is the
write-side of the new "won't fix / acknowledged intrinsic" lane —
called by `/heal-code-review` once it concludes a finding is
intrinsic complexity. Records the team's decision into
`.heal/findings/accepted.json` (tracked, mirrors `fixed.json` in
shape). Both subcommands stay hidden from `--help`; humans drive
them via the skills.

The renderer / drain-queue integration (filtering accepted findings
out of `Population:` and the T0 / T1 tiers) lands in the next
release entry below.

### ⚠ BREAKING: `.heal/findings/` is now git-tracked

`fixed.json`, `regressed.jsonl`, and `latest.json` are tracked
alongside `config.toml` and `calibration.toml` so teammates on the
same commit see identical drain queues without re-scanning. The
`.heal/.gitignore` template no longer excludes `findings/` — re-run
`heal init --force` to refresh it, then commit the resulting
findings cache.

To make the tracked file byte-stable, `FindingsRecord` no longer
stores wall-clock metadata:

- `id` is now a deterministic 16-hex FNV-1a digest of `(head_sha,
  config_hash, worktree_clean)` (was: ULID).
- `started_at` is removed (was: `Utc::now()` at scan time).
- `RegressedEntry.regressed_at` now records when the regression
  was *detected* (was: when the record was assembled — same wall
  clock, same value).

`heal status` and `heal diff` JSON output drops `started_at` /
`from_started_at` / `to_started_at` accordingly. Skills that surfaced
those fields should switch to `head_sha` for "what state is this".

`heal status` cache reuse now goes through `is_fresh_against`
(`(head_sha, config_hash, worktree_clean)`); a `latest.json` from a
different commit, different config, or a dirty scan auto-refreshes
without `--refresh`. Previously a stale `latest.json` would persist
until the user passed `--refresh` explicitly.

### `heal-config` Strict-fit check

The skill now compares the codebase's calibration against the Strict
recipe before offering it as a strictness option. When
`Strict.floor_ok` (for CCN or Cognitive) sits above the codebase's
`p95`, the percentile cascade lands every value barely above the
gate at Critical — flooding the drain queue with proxy-metric noise
instead of surfacing real targets.

When the check fires, the strictness question's Strict option gets
a warning preface naming the metrics and the numbers, so the user
sees the trade-off before picking. The warning is advisory; Strict
remains pickable for domains (cryptography, safety-critical) where
"every function above CCN=8 is Critical" is the actual goal.

### Two-tier drain summary in `heal status` and `heal diff`

`heal status` and `heal diff` now foreground the drain queue ahead of
the raw severity distribution, so the user sees "what to fix" before
"how big is the population".

`heal status` header changes from a single line to two:

```
  Drain queue: T0 6 findings (4 files)  ·  T1 27 findings (15 files)
  Population:  [critical] 25   [high] 27   [medium] 421   [ok] 1577
```

The T0 / T1 sizes are computed from the active `[policy.drain]`; raw
severity counts move to a "Population:" line that frames them as
distribution context, not a goal.

`heal diff` similarly splits its progress block:

```
  Progress (T0 drain):  3 / 6 resolved → 50% complete
  Population:           112 / 2050 resolved (6%)
```

The "Progress" number is now scoped to the must-drain tier, so it
reads like real progress against the actionable queue. The wider
population ratio (still computed for back-compat) becomes
secondary — much of it is Advisory churn (`change_coupling.expected`
on docs cross-mentions, etc.) and was never a target.

`DiffReport` JSON gains three additive fields — `t0_resolved`,
`t0_total`, `t0_progress_pct` — and `DiffEntry` gains `from_hotspot`
so consumers can compute baseline-side T0 counts precisely (the
existing `hotspot` field is curr-side biased and stays for
back-compat). `progress_pct` keeps its previous meaning (resolved /
baseline-total over the full population). New fields are
`#[serde(default)]` so older payloads parse cleanly.

`PolicyDrainConfig` exposes a `tier_for_attrs(metric, severity,
hotspot)` helper for callers that don't have a full `Finding` (used
by `heal diff` to classify `DiffEntry`s).

### `heal diff` defaults to the calibration baseline

Bare `heal diff` (no positional ref) now resolves to the SHA recorded
in `calibration.toml` as `meta.calibrated_at_sha` — the commit at
which `heal init` (or the most recent `heal calibrate --force`)
captured the project's percentile breaks. Falls back to `HEAD` when
no baseline SHA is recorded (e.g. a calibration produced outside a
git worktree). Pass `heal diff HEAD` for the previous behaviour.

The motivation is read-naturalness: "Progress: N% complete" should
mean "drained since calibration", not "since the last commit".

### `heal diff` and `heal metrics` honour `$PAGER`

Both commands now pipe through `$PAGER` (default `less`) when stdout
is a terminal — same convention as `heal status`. Both gain a
`--no-pager` flag to opt out; `--json` always writes raw to stdout
regardless. The pager helper now lives in `core::term` and is shared
across the three commands.

The leading and trailing `── HEAL diff ────` divider lines are gone
— a pager already delimits the screen, and the title was redundant.

### `heal init` workspace summary

Renamed the post-init manifest hint from `Monorepo detected:` to
`Workspace detected:` and enriched it: for Cargo `[workspace]
members` and npm `workspaces` arrays, init now enumerates each
declared member directory and labels it with its auto-detected
primary language. Manifests we can't parse without extra dependencies
(pnpm yaml, go.work, Nx / Turbo) still show the presence-only line.

The `init --json` payload's `monorepo_signals[]` entries gain an
optional `members: [{ path, primary_language? }, ...]` array (omitted
when empty, so existing skill consumers see no change).

The `→ goal: bring [critical] to 0` nudge is gone — per scope.md R1,
metrics are proxies, not targets, and the line conflicted with that
framing. The "Next steps" block now also shows `heal diff` and, when
skills were just installed, the example slash commands
(`claude /heal-config`, `/heal-code-review`, `/heal-code-patch`).

### ⚠ BREAKING — `exclude_paths` is now `.gitignore` syntax

`git.exclude_paths`, `metrics.loc.exclude_paths`, and
`[[project.workspaces]].exclude_paths` previously matched as
case-sensitive **substring** patterns. They now parse as
**`.gitignore`** lines with the full DSL: glob (`*`, `**`, `?`,
`[abc]`), directory-only (`foo/`), root anchoring (`/foo`), negation
(`!keep`), and `#` comments.

**Migration:** most existing configs work without changes. Patterns
that relied on bare keyword substring behaviour need a small edit:

| Old (substring) | New (gitignore) | Why |
|---|---|---|
| `target/` | `target/` (unchanged) | Directory pattern works the same |
| `vendor` | `vendor/` *or* `vendor/**` | Bare keyword used to match `weird-vendor-stuff/`; gitignore matches a file/dir literally named `vendor` only |
| `pkg/web/vendor/` | `pkg/web/vendor/` (unchanged) | Anchored directory pattern works the same |
| `.test.ts` | `*.test.ts` (suffix) *or* `**/.test.ts` (exact basename) | Substring matched any path containing the literal `.test.ts` *anywhere* — usually the user's intent is "files whose name ends in `.test.ts`", so `*.test.ts` is the typical replacement; gitignore basename-globs are unanchored by default so no leading `**/` is needed |

`heal status --refresh` after the upgrade reports the new
`severity_counts`; if a previously-excluded subtree starts surfacing
findings, the cause is almost always a bare-keyword pattern that
needs `/` or `*` decoration.

`Config::validate` (run on every config load) now also verifies each
exclude line parses as gitignore syntax. Malformed patterns surface
as `ConfigInvalid` schema errors before any scan starts.

### Workspace `exclude_paths` is wired

`[[project.workspaces]].exclude_paths` was previously declared in the
schema but inert at scan time. It now applies, scoped to the
declaring workspace via gitignore-line translation:

- `vendor/` under `path = "pkg/web"` → matches `pkg/web/**/vendor/`
- `/dist` (anchored to workspace root) → `/pkg/web/dist`
- `!keep.log` → `!pkg/web/**/keep.log`

Other workspaces are unaffected.

### `heal metrics --workspace <PATH>`

New flag scopes every observer to a sub-path. Loc walks only that
subtree; walk-based observers (Complexity / Lcom / Duplication) drop
out-of-workspace files; git-based observers (Churn / ChangeCoupling)
recompute `commits_considered` against the in-workspace universe so
lift / churn totals stay consistent.

### Hotspot graduation floor

`HotspotCalibration` gains `floor_ok: Option<f64>` (default
`FLOOR_OK_HOTSPOT = 22 = 2 × FLOOR_OK_CCN`). Composite scores
strictly below the floor never flag as hotspots even when they sit
in the top decile of a uniformly-cold codebase. Override per project
via `[metrics.hotspot] floor_ok = 50.0`. Legacy snapshots written
before v0.3+ have `floor_ok = None` and fall back to pure
percentile-rank behaviour.

### Per-workspace calibration floor overrides

`[[project.workspaces]] [project.workspaces.metrics.<metric>]
floor_critical = N` / `floor_ok = N` overrides apply *after* the
global `[metrics.<m>]` overrides for that workspace's calibration
table. Useful when one workspace runs cleaner or legacier than the
rest:

```toml
[[project.workspaces]]
path = "pkg/legacy"

[project.workspaces.metrics.ccn]
floor_critical = 40
floor_ok = 18
```

Supported metrics: `ccn`, `cognitive`, `duplication`,
`change_coupling`, `lcom`. Scan-time tunables (`since_days`,
`min_coupling`) remain global for now.

### Expected coupling Advisory bucket

`change_coupling` pairs classified as `TestSrc` (test ↔ source) or
`DocSrc` (doc ↔ source) used to be silently dropped from the drain
queue. They now emit `change_coupling.expected` Findings at
`Severity::Medium` so users can see what was demoted under
`heal status --all` (Advisory tier). The pairs still don't enter
the drain queue — Medium routes to Advisory by default.

### Cross-workspace coupling Advisory bucket

`change_coupling` pairs whose endpoints belong to *different*
declared workspaces are retagged
`change_coupling.cross_workspace` and parked in the Advisory tier by
default. Configurable via
`[metrics.change_coupling] cross_workspace = "surface" | "hide"`.

## v0.2.1 — 2026-05-01

### Fixes
- **Skills wire into Claude Code automatically.** `heal init` and
  `heal skills install` now register the bundled plugin via a
  local marketplace entry in `.claude/settings.json`, so the
  `/heal-code-check` / `/heal-code-fix` skills are discoverable
  without a manual install step (`bba9acf`).
- **Post-commit nudge fits on one line.** The Severity summary now
  prints as a single colored row (`a46cfd7`) — the multi-line v0.2.0
  format was awkward in busy commit terminals.

### Chore
- Bump `toml` 0.8.23 → 1.1.2+spec-1.1.0 (`b2e3bfe`).

## v0.2.0 — 2026-05-01

The Severity-aware release. v0.1.0 produced metric numbers; v0.2.0
turns them into Findings classified against per-codebase
percentile breaks, with a fix-drain skill and a post-commit nudge.

### Features

**Severity + calibration**
- `heal calibrate` derives per-metric percentile breaks (p50/p75/p90/
  p95) from the current codebase, plus literature-anchored absolute
  floors (`FLOOR_CCN = 25`, `FLOOR_COGNITIVE = 50`,
  `FLOOR_DUPLICATION_PCT = 30`). Output written to
  `.heal/calibration.toml` (`a43fdef`, `f636d2a`).
- Findings carry a four-step `Severity` ladder (`Ok`, `Medium`,
  `High`, `Critical`) plus a `hotspot` decoration for files in the
  top 10% by Hotspot score (`7db0570`).
- `heal check` (Severity TUI) plus `heal cache` (mark / browse the
  fix queue) ship as the user-facing surface (`cb46519`).
- `Severity` counts surface on every commit via the post-commit
  nudge (`e45d327`) — replaces the v0.1 SessionStart approach.

**Drain skill**
- `/heal-fix` Claude skill drains the findings cache one fix per
  commit in Severity order, refusing dirty worktrees (`60125d5`).
- `/heal-fix` consolidated with the per-metric `check-*` skills into
  the `/heal-code-check` + `/heal-code-fix` pair, with a
  language-aware drain flow (`bace1ca`).

**New languages**
- JavaScript (`.js` / `.jsx`) (`ed88f93`).
- Python (`.py` / `.pyi`) (`ed15dfd`).
- Go (`.go`) — LCOM deferred to v0.3+ (`f1adbfd`).
- Scala (`.scala` / `.sc`) — LCOM deferred to v0.3+ (`21267be`).

**LCOM and coupling**
- LCOM approximation (per-class cohesion clusters via union-find) with
  configurable `min_cluster_count` (`64a848c`, `fe2ef30`, `a6f88bb`).
- Change Coupling pairs split into `Symmetric` (both directions
  strong) vs `OneWay { from, to }` based on conditional probability
  asymmetry (`8afba7a`).

**Architecture**
- `Feature` trait + `FeatureRegistry` migrate the per-metric
  classify/decorate pipeline to a pluggable form (`532d305`,
  `aff78af`).
- Result cache shape: `.heal/checks/` (typed records, fix-state
  reconciliation) (`85637ea`).
- Event-log compaction: gzip at 90 days, delete at 365 days
  (`1b5665b`, `bf79c0b`).

**CLI ergonomics**
- `heal logs` / `heal snapshots` / `heal checks` split into
  browse-only commands; `heal fix` retained for fix-state mutation
  (`7144a7e`).
- `heal fix diff` reframed in git-style positional form (no
  `--worktree`) (`a7b848a`).
- `heal init` offers inline Claude skill install with a structured
  install summary (`3234275`); `--force` propagates to the
  bundled-skill refresh path (`33731b1`).
- Pre-commit `rustfmt` hook added under `.githooks/` (`f1d8fe8`).

### Chore
- `thiserror` 1.0 → 2.0 (`c2a069a`).
- Tree-sitter grammar bumps (Go, JavaScript, Python, Scala).
- Astro 5 → 6 + Starlight breaking changes for `docs/`
  (`cb476d9`, `caa89db`, `b5ebf11`).
- TypeScript 5 → 6.0.3 in `docs/`, then pinned 5.9.3 for
  Pages action compat (`31cbe09`, `21d7f0a`).
- Slim logo + favicon (`5bf4c57`).
- CI: docs build only on push to `main`, drop pull_request trigger
  (`3c37303`).

## v0.1.0 — 2026-04-29

Initial public release. The observe half of the loop: read code
health out of any project, write structured snapshots and
recommendations, surface them through CLI + Claude Code skills.

### Features

**Observer pipeline**
- `tokei` integration for LOC and language inventory (`4ce0c3c`).
- Tree-sitter parsing foundation with CCN and Cognitive Complexity
  per function (`3139b00`).
- Rust language support, wired into `ComplexityObserver`
  (`9d3b0dd`).
- Churn, Change Coupling, Duplication, Hotspot composition observers
  (`0528d89`).
- `MetricsSnapshot` writer with worst-N rendering and per-language
  feature gates (`97b7093`).

**Configuration**
- Per-metric `top_n` overrides with a global fallback (`621b7c4`).

**CLI**
- `heal init` — language detection, config write, post-commit hook
  install, initial scan (`3cb23b0`).
- `heal hook commit | edit | stop` — Claude Code hook entry points
  routed through a generic `hook` command (`2150298`).
- `heal status` — render the latest `MetricsSnapshot`.
- `heal check` — streaming progress, plain-text and JSON output
  (`76dcf0d`).
- `heal logs` — browse the structured event log.

**Claude Code integration**
- SessionStart nudge with severity-aware messaging.
- Drift-aware skills install / update (`fb33201`).
- Per-metric `check-*` skills + `heal status --metric` filter
  (`2fb2f9d`).

### Packaging
- Workspace collapsed into a single `heal-cli` crate so
  `cargo install heal-cli` is the supported install path
  (`8559a6d`).
- `cargo-dist` scaffolding for binary releases (`cebaa2e`).
- LICENSE (MIT), README, CLAUDE.md added ahead of OSS publication
  (`2555e70`).

### Pre-release polish
- Bug fixes, dead-config sweep, dual-license metadata (`2ae4eb2`).
