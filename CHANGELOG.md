# Changelog

## Unreleased

### Features

- **JavaScript, Python, Go, and Scala join the default-enabled
  grammars.** The released binary (Homebrew, shell installer,
  `cargo install heal-cli`) now ships with all six tree-sitter
  parsers — `lang-js`, `lang-py`, `lang-go`, `lang-scala` move from
  opt-in to default alongside the existing `lang-ts` and
  `lang-rust`. Complexity (CCN + Cognitive) and Duplication run on
  all six. LCOM stays scoped to TypeScript / JavaScript / Python /
  Rust — Go has no class scope and Scala awaits the LSP backend
  (v0.5+).
- **Cargo feature names switched to long form as canonical.**
  `lang-typescript`, `lang-javascript`, `lang-python` now match the
  upstream `tree-sitter-<name>` parser crate names; source-level
  `cfg(feature = "lang-...")` gates and the CI matrix follow suit.
  Short forms `lang-ts` / `lang-js` / `lang-py` remain as aliases so
  existing `cargo build --features lang-ts` invocations keep
  working. (`lang-go`, `lang-scala`, `lang-rust` were already in
  long form.)

## v0.3.1 — 2026-05-03

### Fixes

- **`cargo publish` ships the bundled skill set again.** The
  `crates/cli/Cargo.toml` `include = [...]` allow-list still
  referenced the retired `plugins/**/*` path after v0.3.0's
  `crates/cli/skills/` flatten, so the published tarball missed
  `skills/` and `include_dir!` panicked during the verify step
  (`error: proc macro panicked … "skills" is not a directory`).
  v0.3.0's binary, GitHub Release, and Homebrew artefacts all
  shipped fine; v0.3.1 is a crates.io-only re-publish with the
  include allow-list pointing at `skills/**/*`.

## v0.3.0 — 2026-05-03

The CLI-shape and monorepo-aware release. The user-facing surface
(`heal status` / `heal metrics` / `heal diff` / `heal mark fix`) is
now stable; the cache is now a single tracked record per repo;
monorepos are first-class with per-workspace calibration; and
findings the team has decided are intrinsic can be parked in
`accepted.json` instead of cluttering the drain queue forever.

### ⚠ BREAKING

#### CLI rename: `status` / `metrics` / `diff` / `mark fix` are now stable

The v0.2 names flipped roles:

| v0.2                | v0.3                     | What it does                                     |
| ------------------- | ------------------------ | ------------------------------------------------ |
| `heal check`        | `heal status`            | Render the cached `FindingsRecord`               |
| `heal status`       | `heal metrics`           | Per-metric one-shot recompute                    |
| `heal fix diff`     | `heal diff <git-ref>`    | Diff vs a ref (default: calibration baseline)    |
| `heal fix mark`     | `heal mark fix` (hidden) | Skill-only; agent-driven fix recorder            |
| `heal fix list`     | (removed)                | Read `.heal/findings/latest.json` directly       |

**Migration:** rename invocations in scripts and CI, and run
`heal skills update` so the bundled skills stop referencing the old
names. `heal mark-fixed` (the v0.2.x interim form) still works as
a hidden alias that prints a one-line stderr deprecation warning.

#### `.heal/findings/` is git-tracked

`fixed.json`, `regressed.jsonl`, `latest.json`, and the new
`accepted.json` are all tracked alongside `config.toml` and
`calibration.toml` so teammates on the same commit see identical
drain queues without re-scanning. The `.heal/.gitignore` template
no longer excludes `findings/` — run `heal init --force` to refresh
it, then commit the resulting findings cache.

To make `latest.json` byte-stable, `FindingsRecord` drops wall-clock
metadata:

- `id` is now a deterministic 16-hex FNV-1a digest of `(head_sha,
  config_hash, worktree_clean)` (was: ULID).
- `started_at` is removed (was: `Utc::now()` at scan time).
- `RegressedEntry.regressed_at` now records when the regression
  was _detected_ (was: when the record was assembled).

`heal status` and `heal diff` JSON drop `started_at` /
`from_started_at` / `to_started_at`. Skills that surfaced those
fields should switch to `head_sha`. Cache reuse now goes through
`is_fresh_against` so a `latest.json` from a different commit,
different config, or dirty scan auto-refreshes without `--refresh`.

#### `FindingsRecord` schema v1 → v2

`FindingsRecord` was renamed from `CheckRecord`; `check_id` →
`id`, `regressed_check_id` → `regressed_in_record_id`. Bumped to
schema v2; v1 caches deserialise as `Ok(None)` and the next
`heal status` rewrites them under v2.

#### Snapshots gone, single-record cache

`.heal/snapshots/` is removed — no more historical metric stream,
no more `heal compact`, no more 90-day gzip / 365-day delete cycle.
The cache is one record (`latest.json`) plus the bounded
`fixed.json` map and the append-only `regressed.jsonl` audit trail.
Use `heal diff <ref>` for drift on demand.

The `heal logs` / `heal snapshots` / `heal checks` browse commands
are removed alongside.

#### `exclude_paths` is gitignore syntax

`git.exclude_paths`, `metrics.loc.exclude_paths`, and
`[[project.workspaces]].exclude_paths` previously matched as
case-sensitive **substring** patterns. They now parse as
**`.gitignore`** lines with the full DSL: globs (`*`, `**`, `?`,
`[abc]`), directory-only (`foo/`), root anchoring (`/foo`),
negation (`!keep`), and `#` comments.

**Migration:** most existing configs work unchanged. Patterns that
relied on bare-keyword substring behaviour need a small edit:

| Old (substring)   | New (gitignore)                                  | Why                                                                                                                                                                  |
| ----------------- | ------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `target/`         | `target/` (unchanged)                            | Directory pattern works the same                                                                                                                                     |
| `vendor`          | `vendor/` _or_ `vendor/**`                       | Bare keyword used to match `weird-vendor-stuff/`; gitignore matches a file/dir literally named `vendor` only                                                         |
| `pkg/web/vendor/` | `pkg/web/vendor/` (unchanged)                    | Anchored directory pattern works the same                                                                                                                            |
| `.test.ts`        | `*.test.ts` _or_ `**/.test.ts` (exact basename)  | Substring matched any path containing `.test.ts` _anywhere_; gitignore basename-globs are unanchored by default so the leading `**/` is usually unnecessary          |

`Config::validate` now verifies each line parses as gitignore
syntax — malformed patterns surface as `ConfigInvalid` schema
errors at load time rather than mysteriously matching nothing.

#### Skill drift derived from frontmatter bytes

`skills-install.json` is gone. Drift detection compares
`canonical(on-disk SKILL.md)` (frontmatter `metadata:` block
stripped) against the bundled raw bytes — no more sidecar manifest,
no more cross-machine drift verdicts diverging because the manifest
was last touched on a different machine. `heal skills install` /
`update` / `status` use the byte comparison directly.

### Features

#### Monorepo / workspace support

- `WorkspaceOverlay` schema: `[[project.workspaces]]` declares a
  monorepo segment; findings under a declared `path` get
  `Finding.workspace = "<path>"` so per-workspace JSON shapes round-
  trip cleanly.
- Per-workspace calibration tables: each declared workspace gets
  its own percentile breaks, so a strict `pkg/web` and a legacy
  `pkg/legacy` calibrate independently. Floor overrides
  (`floor_critical` / `floor_ok` per metric) layer on top of the
  global `[metrics.<m>]` overrides.
- `--workspace <path>` filter on `heal status`, `heal diff`, and
  `heal metrics` — every observer scopes to the subtree (Loc walks
  only that path; Complexity / Lcom / Duplication drop
  out-of-workspace files; Churn / ChangeCoupling recompute
  `commits_considered` against the in-workspace universe).
- `WorkspaceOverlay.exclude_paths` now applies at scan time,
  scoped to the declaring workspace via gitignore translation
  (`vendor/` under `path = "pkg/web"` → `pkg/web/**/vendor/`).
- `change_coupling` pairs whose endpoints straddle two declared
  workspaces are retagged `change_coupling.cross_workspace` and
  parked in Advisory by default. Configurable via
  `[metrics.change_coupling] cross_workspace = "surface" | "hide"`.
- `heal init` post-scan hint renamed `Monorepo detected:` →
  `Workspace detected:`, now enumerates Cargo `[workspace] members`
  and npm `workspaces` directories with their auto-detected primary
  language. The `init --json` payload's `monorepo_signals[]` entries
  gain an optional `members: [{ path, primary_language? }, ...]`
  array.
- `/heal-config` skill gains a workspace setup phase that detects
  the manifest, proposes `[[project.workspaces]]` blocks per
  member, and runs the strictness recipe per workspace.

#### Accepted findings (`heal mark accept`)

- `heal mark accept --finding-id <ID> [--reason <TEXT>]` records a
  "won't fix / acknowledged intrinsic" decision into
  `.heal/findings/accepted.json` (tracked, mirrors `fixed.json` in
  shape). Distinct from `fix` — accepted entries persist across
  re-detections by design.
- `heal status`, `heal diff`, and the post-commit nudge exclude
  accepted findings from the drain queue (T0 / T1), the
  `Population:` severity counts, and the "X critical, Y high"
  nudge. A new `Accepted: N findings (M files)` line surfaces in
  the `heal status` header; `--all` adds a `📌 Accepted` section
  for the audit trail.
- `Finding` JSON gains `accepted: bool` (additive); `DiffEntry`
  gains `from_accepted: bool`.
- `/heal-code-review` proposes the exact `heal mark accept`
  invocation when triage classifies a finding as Intrinsic or
  Cohesive procedural, with documented "accept (per-finding) vs
  exclude_paths (per-file/tree)" guidance. `/heal-code-patch`
  skips accepted findings from the drain loop.

#### `heal mark` group

`heal mark-fixed` is replaced by `heal mark fix` (sibling to
`heal mark accept`). The legacy form prints a one-line stderr
deprecation warning and delegates so v0.2 skill bundles keep
running until `heal skills update`. Both subcommands stay hidden
from `--help`; humans drive them via the skills.

#### `heal diff` improvements

- `heal diff <git-ref>` runs the observer pipeline against a
  transient `git worktree` materialised at the requested ref and
  diffs the resulting `FindingsRecord` against the live one. The
  baseline applies _today's_ rules to historical source so the
  comparison is apples-to-apples.
- LOC ceiling: bare repo size > `[diff].max_loc_threshold`
  (default 200_000) returns exit 2 with guidance to drive the
  worktree pair by hand, so the cost stays bounded.
- Bare `heal diff` (no positional ref) defaults to the SHA recorded
  in `calibration.toml` as `meta.calibrated_at_sha`. Falls back to
  `HEAD` when no baseline SHA is recorded.
- New `Progress (T0 drain)` line scopes the percentage to the
  must-drain tier; the wider `Population:` ratio stays as
  back-compat secondary signal. `DiffReport` JSON gains
  `t0_resolved`, `t0_total`, `t0_progress_pct`, and `DiffEntry`
  gains `from_hotspot` so consumers can compute baseline-side T0
  counts precisely.

#### Two-tier drain summary in `heal status`

`heal status` foregrounds the drain queue ahead of the raw
severity distribution:

```
  Drain queue: T0 6 findings (4 files)  ·  T1 27 findings (15 files)
  Population:  [critical] 25   [high] 27   [medium] 421   [ok] 1577
  Accepted:    1 findings (1 files)
```

T0 / T1 sizes come from the active `[policy.drain]`. The
`Accepted:` line only appears when the team has accepted any
findings.

#### Hotspot graduation floor

`HotspotCalibration` gains `floor_ok: Option<f64>` (default
`FLOOR_OK_HOTSPOT = 22 = 2 × FLOOR_OK_CCN`). Composite scores
strictly below the floor never flag as hotspots even when they sit
in the top decile of a uniformly-cold codebase. Override per
project via `[metrics.hotspot] floor_ok = 50.0`.

#### Expected coupling Advisory bucket

`change_coupling` pairs classified as `TestSrc` (test ↔ source) or
`DocSrc` (doc ↔ source) now emit `change_coupling.expected`
Findings at `Severity::Medium` so users can see what was demoted
under `heal status --all` (Advisory tier). The pairs still don't
enter the drain queue.

#### `heal-config` Strict-fit warning

The skill compares the codebase's calibration against the Strict
recipe before offering it as a strictness option. When
`Strict.floor_ok` for CCN or Cognitive sits above the codebase's
`p95`, the percentile cascade lands every barely-above-floor value
at Critical — flooding the drain queue. The Strict option now
carries a warning preface naming the metrics and numbers when this
fits poorly, so the user sees the trade-off before picking. Strict
remains pickable for domains (cryptography, safety-critical) where
"every function above CCN=8 is Critical" is the actual goal.

#### Pager + summary at top

`heal status` renders the summary block (Drain queue, Population,
Accepted) before the per-Severity sections, and pipes through
`$PAGER` (default `less`) when stdout is a terminal. `heal diff`
and `heal metrics` adopt the same convention. `--no-pager` opts
out; `--json` writes raw to stdout regardless. Leading / trailing
`── HEAL ────` divider lines are gone — the pager already delimits
the screen.

### Fixes

- **`heal status` ↔ `heal metrics` polish.** Dogfooded output
  cleanups: trailing whitespace, spurious blank lines in the LCOM
  per-class block, missing thousands separators in metrics summary
  totals (`69ef794`).
- **CLI rename sweep.** A handful of `status` / `metrics`
  conflations missed the rename pass landed in follow-up
  (`e73c537`).

### Chore

- **Bundled skills tracked.** `heal init` extracts skills under
  `.claude/skills/heal-*/` on first run; the directory is now
  tracked in this repo so dogfood + CI see the same content.
- **Internal docs and rules.** `.claude/docs/` (descriptive
  architecture / data-model / commands / observers / glossary) and
  `.claude/rules/` (prescriptive scope / terminology / workflow /
  invariants / skills-and-hooks) split from `CLAUDE.md` so the
  agent-facing reference scales without bloating the project
  preamble.
- **Internal comments are English.** Source comments (`//`, `///`,
  `//!`, `;` in `.scm`, `#` in `Cargo.toml` / shell hooks) are now
  uniformly English; rule codified in `.claude/rules/workflow.md`
  R6.1.
- **User docs rewrite.** Starlight pages cover the new CLI surface
  + monorepo + accepted lane, and the Japanese mirror tracks them
  with a CJK-spacing pass.

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
