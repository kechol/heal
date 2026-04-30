# heal

> **h**ook-driven **e**valuation & **a**utonomous **l**oop — a code-health
> harness that turns codebase decay signals into work for AI coding agents.

LLM coding agents are usually reactive: a human files a task before the
agent moves. Codebases, meanwhile, decay continuously — complexity creeps,
hotspots shift, duplicates accumulate. heal closes that gap by turning
**codebase state changes** into **agent triggers**.

The loop is observe → calibrate → check → fix:

- **observe**: every commit's metrics land in `.heal/snapshots/`.
- **calibrate**: codebase-relative percentiles (`p75 / p90 / p95`) plus
  absolute floors define a 4-tier Severity ladder
  (`Critical / High / Medium / Ok`). Hotspot is an orthogonal flag.
- **check**: `heal check` classifies findings by Severity, writes a TODO
  cache to `.heal/checks/`, and groups the rendered list per-file.
- **fix**: the bundled `/heal-fix` Claude skill drains the cache one
  finding per commit; the post-commit hook surfaces the next
  Critical / High items right inside `git commit` output.

Documentation: <https://kechol.github.io/heal/>

> ⚠️ **Status: v0.2 in progress.** macOS / Linux only. The
> `.heal/checks/` schema and the `heal check` / `heal fix` surfaces
> stabilised in v0.2; some doc / coverage features land in v0.3.

## What it measures

Every metric is computed by an observer under `src/observer/` and
persisted on the post-commit hook to `.heal/snapshots/YYYY-MM.jsonl`.
Calibration assigns a Severity to each Finding using the codebase's own
distribution.

| Metric                       | What it captures                                                                                                | Languages         | Calibration |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------- | ----------------- | :---------: |
| LOC                          | Lines of code per language; primary-language detection (`tokei`)                                                | language-agnostic |             |
| CCN                          | McCabe Cyclomatic Complexity per function (tree-sitter)                                                         | TypeScript, Rust  | ✓           |
| Cognitive                    | Sonar-style Cognitive Complexity (nesting + flow-break weighting)                                               | TypeScript, Rust  | ✓           |
| Churn                        | Per-file commit frequency and added/deleted line totals over a `since_days` window                              | language-agnostic |             |
| Change Coupling              | Co-change pair counter (`code-maat` style); bulk-commit cap; sum-of-coupling per file                           | language-agnostic | ✓           |
| Change Coupling (symmetric)  | Subset of pairs where `min(P(B\|A), P(A\|B)) ≥ symmetric_threshold` — a "responsibility mixing" signal          | language-agnostic | ✓           |
| Duplication                  | Type-1 (exact) clones via tree-sitter leaf tokens + Rabin-Karp                                                  | TypeScript, Rust  | ✓           |
| Hotspot                      | `commits × ccn_sum × weights` composition over Churn + CCN; flag (top-10%), not Severity                        | TypeScript, Rust  | ✓ (flag)    |
| LCOM                         | Lack of Cohesion of Methods — per-class disjoint method clusters (`tree-sitter-approx`)                         | TypeScript, Rust  | ✓           |

Per-language Cargo features (`lang-ts`, `lang-rust`) decide which
parsers compile in; the default build enables both.

## Install

macOS and Linux are supported.

### Homebrew

```sh
brew install kechol/tap/heal-cli
```

### Cargo

```sh
cargo install heal-cli
```

### Shell installer (pre-built binary)

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/kechol/heal/releases/latest/download/heal-cli-installer.sh | sh
```

Verify the install:

```sh
heal --version
```

## Quick Start

Inside any git repository:

```sh
heal init                # create .heal/, calibrate, install the post-commit hook
heal check               # analyze + render the Severity-grouped TODO list
heal skills install      # bundle the Claude plugin for /heal-fix
```

After `heal init`, every commit:

1. Runs every observer (a single `run_all` pass — the snapshot writer
   and the post-commit nudge share the result).
2. Appends a `MetricsSnapshot` to `.heal/snapshots/` and a `CommitInfo`
   metadata record to `.heal/logs/`.
3. Prints any `Critical` / `High` finding to stdout (`Medium` and `Ok`
   stay quiet) so the next move is visible inside the commit output.

`heal check` then re-runs the analysis on demand and writes a
`CheckRecord` to `.heal/checks/latest.json` — the single file the
`/heal-fix` skill reads as a TODO list.

The full walkthrough is at <https://kechol.github.io/heal/quick-start/>.

## CLI

| Command | Purpose |
|---------|---------|
| `heal init [--force]` | Create `.heal/`, calibrate from the initial scan, write `config.toml` + `calibration.toml`, install the post-commit hook, capture the first snapshot. |
| `heal status [--json] [--metric <name>]` | Metric series + delta from the most recent snapshot. `--metric` scopes output to one observer (`loc` / `complexity` / `churn` / `change-coupling` / `duplication` / `hotspot` / `lcom`). |
| `heal logs [--since <RFC3339>] [--filter <event>] [--limit N] [--json]` | Stream `.heal/logs/` (commit / edit / stop hook events). |
| `heal snapshots [--since <RFC3339>] [--filter <event>] [--limit N] [--json]` | Stream `.heal/snapshots/` (`MetricsSnapshot` + `calibrate` events). |
| `heal checks [--since <RFC3339>] [--limit N] [--json]` | Newest-first summary of every `CheckRecord` in `.heal/checks/`. |
| `heal check [--metric <name>] [--severity <level>] [--feature <prefix>] [--all] [--top N] [--json] [--refresh]` | Render the cached `CheckRecord` from `.heal/checks/latest.json`. Re-scans only on a missing cache or `--refresh`; that path is the single writer of `.heal/checks/`. |
| `heal fix show <check_id> [--json]` | Detail-render one cached record (use `--json` for the stable shape). |
| `heal fix diff [<from>] [<to>] [--worktree] [--all] [--json]` | Bucket findings into Resolved / Regressed / Improved / New / Unchanged across two records, with progress %. `--worktree` scans the live tree without writing a record. |
| `heal fix mark --finding-id <id> --commit-sha <sha>` | Append a `FixedFinding` line; called by `/heal-fix` after each commit. |
| `heal calibrate [--force]` | Calibrate codebase-relative Severity thresholds. Default reads `.heal/calibration.toml` and reports drift triggers; `--force` rescans and overwrites. |
| `heal hook <commit\|edit\|stop>` | Hook entrypoint invoked by git or the Claude plugin. Not for direct use. |
| `heal skills <install\|update\|status\|uninstall>` | Manage the bundled Claude plugin under `.claude/plugins/heal/`. |

Run `heal --help` or `heal <subcommand> --help` for full argument details.

## Configuration

`heal init` writes `.heal/config.toml`. Every metric ships with sensible
defaults; the file is mostly there to override them. The schema is
strict (`deny_unknown_fields`) so typos surface as errors rather than
silently dropping settings. `.heal/calibration.toml` is a sibling file
generated by `heal init` / `heal calibrate` and **not intended for
hand-editing** — only `floor_critical` overrides belong in `config.toml`.

Selected knobs:

```toml
[project]
# Free-form natural language passed to Claude skills. Anything the
# model understands works — "Japanese", "日本語", "ja", "français".
response_language = "Japanese"

[git]
since_days = 90              # Lookback window for churn / change coupling.
exclude_paths = ["dist/"]    # Substrings; matched against every observed path.

[metrics]
top_n = 5                    # Default size of every "worst-N" listing.

[metrics.change_coupling]
enabled = true
min_coupling = 3
# Both P(B|A) and P(A|B) must meet this for a pair to classify as
# Symmetric (responsibility mixing). Lower = looser, higher = stricter.
symmetric_threshold = 0.5

[metrics.duplication]
min_tokens = 50              # Minimum window length for a duplicate block.

[metrics.hotspot]
weight_churn = 1.0
weight_complexity = 1.0
top_n = 8                    # Per-metric override of the global metrics.top_n.

[metrics.lcom]
enabled           = true
backend           = "tree-sitter-approx"   # "lsp" reserved for v0.5+
min_cluster_count = 2                       # ≥2 = class is mechanically separable

[metrics.ccn]
enabled        = true
floor_critical = 25          # Override the McCabe-derived absolute floor.
```

## Severity & Calibration

Severity is computed from each metric's calibrated distribution, not
absolute literature values, so a 200-line script and a 200kloc service
trigger differently for the same raw CCN. The ladder is:

| Tier     | Rule                                              |
|----------|---------------------------------------------------|
| Critical | `value ≥ floor_critical` OR `value ≥ p95`          |
| High     | `value ≥ p90`                                      |
| Medium   | `value ≥ p75`                                      |
| Ok       | otherwise                                          |

`Hotspot` is an orthogonal flag (`score ≥ p90` of the hotspot
distribution). A finding can be `Critical 🔥` (Critical AND hotspot) or
`Critical` (Critical alone) — the renderer surfaces them as separate
buckets.

Recalibration is **never automatic**. The default `heal calibrate`
invocation (no flags) — also surfaced inline by the post-commit nudge
— prints a recommendation when:

- the calibration is over 90 days old,
- the codebase file count has shifted by ±20% since the last calibration,
- or 30 consecutive days have passed with zero Critical findings.

The user runs `heal calibrate --force` to actually rescan; the audit
trail lives in `.heal/snapshots/` as `event = "calibrate"`.

## Repository layout

```
heal/
├── crates/
│   └── cli/
│       ├── src/
│       │   ├── core/          # config, calibration, eventlog, snapshot, finding, hash, paths
│       │   ├── observer/      # LOC, complexity, churn, coupling, duplication, hotspot, lcom
│       │   ├── commands/      # CLI subcommand dispatch
│       │   └── …
│       ├── plugins/heal/      # Claude Code plugin (embedded via include_dir!)
│       └── queries/           # tree-sitter queries for CCN / Cognitive / LCOM
├── plugins → crates/cli/plugins   # convenience symlink for plugin authoring
├── LICENSE-MIT
├── LICENSE-APACHE
└── README.md
```

The crate-level `crates/cli/plugins/heal/` directory is the source of
truth for the Claude plugin tree. It sits inside `heal-cli` so the
crates.io tarball ships it, and is materialised into
`.claude/plugins/heal/` by `heal skills install`.

## Claude plugin

`heal skills install` extracts:

- Two hook scripts wired to `heal hook <event>`:
  - `PostToolUse(Edit|Write|MultiEdit)` → `heal hook edit` (logs only)
  - `Stop` → `heal hook stop` (logs only)
- Five read-only `check-*` skills (`overview`, `hotspots`, `complexity`,
  `duplication`, `coupling`) that pull from `heal status --metric <x>`.
- One write skill `heal-fix` that drains `.heal/checks/latest.json` one
  finding per commit (Severity order; `Critical 🔥` first), in
  Conventional Commits format, with `Refs: F#<finding_id>` trailers.
  Refuses to start on a dirty worktree; never pushes; never amends.

Each installed asset's fingerprint is tracked in `.heal-install.json`,
so `heal skills update` can refresh bundled assets without overwriting
files the user has hand-edited (use `--force` to override).

The pre-v0.2 `SessionStart` nudge has been retired — the post-commit
nudge handles the same role with simpler semantics (no cool-down: the
same problem reappears every commit until it's fixed).

## Development

```sh
cargo build  --workspace
cargo test   --workspace
cargo fmt    --all
cargo clippy --workspace --all-targets -- -D warnings
cargo deny   check
```

CI runs all five on push / PR. `clippy::pedantic = warn` at the
workspace level — new code is expected to pass clippy clean.

### Module layout

The whole CLI lives in a single crate, `heal-cli`, organised internally
as if the original three-layer split were still in place:

- **`src/core/`** — config, calibration, event log, snapshot, finding,
  paths, hash, error types, the `.heal/checks/` cache. Pure data types
  and on-disk formats; no CLI or observer logic.
- **`src/observer/`** — metric collectors (LOC, complexity, churn,
  change coupling, duplication, hotspot, lcom). Stateless: reads the
  project tree and emits structured payloads.
- **`src/commands/` + `src/cli.rs` + `src/main.rs`** — argument
  parsing, command dispatch, hook entrypoints, plugin extraction.

## Contributing

Issues and PRs welcome. The project is in early bootstrap; please open
an issue before introducing a new crate, a new external dependency, or
a schema change to `.heal/`.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](./LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](./LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual-licensed as above, without any additional terms
or conditions.
