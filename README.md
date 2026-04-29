# HEAL

> **H**ook-driven **E**valuation & **A**utonomous **L**oop — a code-health
> harness that turns codebase decay signals into work for AI coding agents.

LLM coding agents are usually reactive: a human files a task before the
agent moves. Codebases, meanwhile, decay continuously — complexity creeps,
hotspots shift, duplicates accumulate. HEAL closes that gap by turning
**codebase state changes** into **agent triggers**.

v0.1 is the **observe** half of the loop: collect metrics on every commit,
expose them through `heal status` / `heal check`, and surface threshold
breaches to Claude Code as a session-start nudge. Autonomous repair
(`heal run`, PR generation) lands later.

> ⚠️ **Status: v0.1.** macOS / Linux only. API may shift before v0.2.

## What it measures

Every metric is computed by a `heal-observer` collector and persisted on
the post-commit hook to `.heal/snapshots/YYYY-MM.jsonl`.

| Metric           | What it captures                                                              | Languages       |
| ---------------- | ----------------------------------------------------------------------------- | --------------- |
| LOC              | Lines of code per language; primary-language detection (`tokei`)              | language-agnostic |
| CCN              | McCabe Cyclomatic Complexity per function (tree-sitter queries)               | TypeScript, Rust |
| Cognitive        | Sonar-style Cognitive Complexity (nesting + logical-chain switches)           | TypeScript, Rust |
| Churn            | Per-file commit frequency and added/deleted line totals over a `since_days` window | language-agnostic (git) |
| Change Coupling  | Co-change pair counter (`code-maat` style); bulk-commit cap; sum-of-coupling per file | language-agnostic (git) |
| Duplication      | Type-1 (exact) clones via tree-sitter leaf tokens + Rabin-Karp                | TypeScript, Rust |
| Hotspot          | `commits × ccn_sum × weights` composition over Churn + CCN                    | TypeScript, Rust |

Per-metric Cargo features (`lang-ts`, `lang-rust`) decide which language
parsers compile in; the default build enables both.

## Install

```sh
git clone https://github.com/kechol/heal
cd heal
cargo install --path crates/cli   # produces the `heal` binary
```

Toolchain: Rust 1.85+. The repo pins the exact version via
[mise](https://mise.jdx.dev) — `mise install` from the project root if you
use it, otherwise any rustup install ≥ 1.85 will work.

> macOS and Linux only for v0.1. The hook scripts and path handling assume
> a POSIX-ish environment.

## Quickstart

Inside any git repository:

```sh
heal init                # creates .heal/{config.toml,snapshots,logs,docs,reports}
                         # and installs .git/hooks/post-commit
heal skills install      # extracts the bundled Claude plugin into .claude/plugins/heal
heal status              # show metric summary and findings
heal logs                # stream the commit / edit / stop event timeline
```

After `heal init`, every git commit triggers the post-commit hook, which
runs the observers and appends one `MetricsSnapshot` to `.heal/snapshots/`
plus a `CommitInfo` (sha, parent, author, subject, file/line counts) to
`.heal/logs/`. Claude Code Edit/Stop hooks (installed by
`heal skills install`) append to the same logs file.

`heal status` reads `snapshots/`; `heal logs` reads `logs/`. The two
directories share a generic event-log format (append-only, month-rotated
JSONL, transparent reads over `.gz` once compaction lands).

## CLI

| Command | Purpose |
|---------|---------|
| `heal init [--force]` | Create `.heal/`, write a default `config.toml`, install the post-commit hook, capture an initial snapshot. |
| `heal status [--json] [--metric <name>]` | Metric summary and most recent finding. `--metric` scopes output to a single observer (`loc` / `complexity` / `churn` / `change-coupling` / `duplication` / `hotspot`). |
| `heal logs [--since <RFC3339>] [--filter <event>] [--limit N] [--json]` | Stream `.heal/logs/` events. |
| `heal check [overview\|hotspots\|complexity\|duplication\|coupling] [-- <claude args>]` | Launch Claude Code (`claude -p`) with the matching read-only `check-*` skill. Anything after `--` forwards verbatim to `claude`. |
| `heal hook <commit\|edit\|stop\|session-start>` | Hook entrypoint invoked by git or the Claude plugin. Not for direct use. |
| `heal skills <install\|update\|status\|uninstall>` | Manage the bundled Claude plugin under `.claude/plugins/heal/`. |

Run `heal --help` or `heal <subcommand> --help` for full argument details.

## Configuration

`heal init` writes `.heal/config.toml`. Every metric ships with sensible
defaults; the config file is mostly there so you can override them. The
schema is strict (`deny_unknown_fields`) so typos surface as errors rather
than silently dropping settings.

Selected knobs:

```toml
[project]
# Free-form natural language passed to `heal check`. Use anything the
# model understands — "Japanese", "日本語", "ja", "français".
response_language = "Japanese"

[git]
since_days = 90              # Lookback window for churn / change coupling.
exclude_paths = ["dist/"]    # Substrings; matched against every observed path.

[metrics]
top_n = 5                    # Default size of every "worst-N" listing.

[metrics.loc]
inherit_git_excludes = true  # Combine git.exclude_paths with metrics.loc.exclude_paths.

[metrics.duplication]
min_tokens = 50              # Minimum window length for a duplicate block.

[metrics.hotspot]
weight_churn = 1.0
weight_complexity = 1.0
top_n = 8                    # Per-metric override of the global metrics.top_n.

[policy.high_complexity_new_function]
action = "report-only"
cooldown_hours = 24
threshold = { ccn = 15, delta_pct = 20 }
```

Policy entries drive per-rule cool-down for the SessionStart nudge today;
the `action` ladder (`report-only` → `notify` → `propose` → `execute`)
becomes meaningful when `heal run` lands.

## Repository layout

```
heal/
├── crates/
│   ├── cli/        # `heal-cli` — CLI binary `heal`
│   ├── core/       # `heal-core` — config, eventlog, snapshot, state, paths
│   └── observer/   # `heal-observer` — LOC, complexity, churn, coupling, duplication, hotspot
├── plugins/
│   └── heal/       # Claude Code plugin (embedded into `heal-cli` via include_dir!)
├── LICENSE-MIT
├── LICENSE-APACHE
└── README.md
```

`plugins/heal/` is the source of truth for the Claude plugin tree. It is
embedded into the `heal` binary at build time and materialised into
`.claude/plugins/heal/` by `heal skills install`.

## Claude plugin

`heal skills install` extracts:

- Three hook scripts wired to `heal hook <event>`:
  - `PostToolUse(Edit|Write|MultiEdit)` → `heal hook edit` (logs only)
  - `Stop` → `heal hook stop` (logs only)
  - `SessionStart` → `heal hook session-start` (cool-down-aware nudge)
- Five read-only `check-*` skills (`overview`, `hotspots`, `complexity`,
  `duplication`, `coupling`) that pull from `heal status --metric <x>`.

Each installed asset's fingerprint is tracked in `.heal-install.json`, so
`heal skills update` can refresh bundled assets without overwriting files
the user has hand-edited (use `--force` to override).

## Development

```sh
cargo build  --workspace
cargo test   --workspace
cargo fmt    --all
cargo clippy --workspace --all-targets -- -D warnings
cargo deny   check
```

CI runs all five on push / PR. `clippy::pedantic = warn` at the workspace
level — new code is expected to pass clippy clean.

### Crate boundaries

- **`heal-core`** — pure data types and on-disk formats (config, eventlog,
  snapshot, state, paths, error). No CLI, no agent invocation, no observer
  logic.
- **`heal-observer`** — metric collectors. Stateless. Reads the project
  tree and emits structured payloads.
- **`heal-cli`** — argument parsing, command dispatch, hook entrypoints,
  plugin extraction. Thin wrapper over `heal-core` and `heal-observer`.

## Contributing

Issues and PRs welcome. The project is in early bootstrap; please open an
issue before introducing a new crate, a new external dependency, or a
schema change to `.heal/`.

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
license, shall be dual-licensed as above, without any additional terms or
conditions.
