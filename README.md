# HEAL

> **H**ook-driven **E**valuation & **A**utonomous **L**oop — a code-health
> harness that turns codebase decay signals into autonomous maintenance work
> for AI coding agents.

> ⚠️ **Status: v0.1 foundation.** The workspace, configuration, event-log
> rotation, and Claude plugin scaffolding are in place. Metric observers and
> the autonomous repair loop land in subsequent releases — see
> [`TODO.md`](./TODO.md).

## Why

LLM coding agents are usually reactive: a human has to file the task before
the agent moves. Codebases, meanwhile, decay continuously — complexity
creeps, hotspots shift, docs fall behind code. HEAL closes that gap by
turning **codebase state changes** into **agent triggers**:

- Observe code-health metrics (hotspot, complexity, churn, duplication, doc
  coverage / skew, …).
- Decide what to surface based on policy (`report-only` → `notify` →
  `propose` → `execute`).
- Hand triggered work to Claude Code (or another agent) via skills.

The full design rationale, metric catalog, OSS tool reference, and policy
patterns live in [`KNOWLEDGE.md`](./KNOWLEDGE.md). Read that first if you
care about the *why*; this README focuses on the *what* and *how*.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  Codebase (source, tests, docs, .git/)                       │
└──────────┬──────────────────────────────────────────────────┘
           ▼
┌─────────────────────────────────────────────────────────────┐
│  Observer  — metric collection (heal-observer crate)         │
└──────────┬──────────────────────────────────────────────────┘
           ▼
┌─────────────────────────────────────────────────────────────┐
│  Aggregator — `.heal/snapshots/YYYY-MM.jsonl` (heal-core)    │
└──────────┬──────────────────────────────────────────────────┘
           ▼
┌─────────────────────────────────────────────────────────────┐
│  Trigger    — policy evaluation, cool-down, dedup            │
└──────────┬──────────────────────────────────────────────────┘
           ▼
┌─────────────────────────────────────────────────────────────┐
│  Executor   — Claude Code skills, `heal run` (v0.2+)         │
└─────────────────────────────────────────────────────────────┘
```

## Install

> v0.1 supports macOS and Linux only. Windows is not on the roadmap before
> v0.5.

Build from source (the only option until the first release):

```sh
git clone https://github.com/kechol/heal
cd heal
cargo install --path crates/cli   # produces the `heal` binary
```

Toolchain: this repo pins Rust via [mise](https://mise.jdx.dev). If you have
mise installed, `cd` into the project and let it resolve the version
(currently Rust 1.95 stable).

## Quickstart

```sh
# Inside any git repository:
heal init                # creates .heal/{config.toml,snapshots,logs,docs,reports}
heal skills install      # extracts the bundled Claude plugin into .claude/plugins/heal
heal status              # show snapshot count and findings
heal logs                # stream the commit/edit/stop event timeline
```

Git post-commit hooks write `MetricsSnapshot` records to
`.heal/snapshots/YYYY-MM.jsonl` and a lightweight commit metadata entry to
`.heal/logs/YYYY-MM.jsonl`. Claude Code Edit/Stop hooks append to the same
logs file. `heal status` reads `snapshots/`; `heal logs` reads `logs/`.

## CLI

| Command | Purpose | Available |
|---------|---------|-----------|
| `heal init` | Create `.heal/` and write a recommended `config.toml` | v0.1 |
| `heal hook <commit\|edit\|stop>` | Hook entrypoint (git or Claude plugin) | v0.1 |
| `heal status [--json]` | Metric summary and recent findings | v0.1 |
| `heal logs [--since RFC3339] [--filter EVENT] [--limit N] [--json]` | Stream `.heal/logs/` events | v0.1 |
| `heal check` | Read-only Claude Code analysis | v0.1 (stub today) |
| `heal skills <install\|update\|status\|uninstall>` | Manage the bundled plugin | v0.1 |
| `heal run` | Apply repairs via Claude Code (PR mode) | v0.2+ |
| `heal docs` | Documentation coverage / preview | v0.3+ |

Run `heal --help` or `heal <subcommand> --help` for argument details.

## Configuration

`heal init` writes `.heal/config.toml`. Every metric has an `enabled` flag
and policy actions follow a four-tier ladder
(`report-only` → `notify` → `propose` → `execute`) so you can opt in to
automation gradually. See `KNOWLEDGE.md` § 11.4 for the full schema and
auto-detection rules.

History records rotate per calendar month
(`.heal/history/YYYY-MM.jsonl`). Reading is transparent over future `.gz`
compaction.

## Repository layout

```
heal/
├── crates/
│   ├── cli/        # `heal-cli` — CLI binary `heal`
│   ├── core/       # `heal-core` — config, history, state, paths, errors
│   └── observer/   # `heal-observer` — metric collectors (skeleton today)
├── plugins/
│   └── heal/       # Claude Code plugin (embedded into `heal-cli` via include_dir!)
├── KNOWLEDGE.md    # Design philosophy + metric catalog + implementation plan
├── TODO.md         # Roadmap (v0.1 → v0.5)
└── CLAUDE.md       # Guidance for Claude Code when working in this repo
```

## Development

```sh
cargo build --workspace
cargo test  --workspace
cargo fmt   --all
cargo clippy --workspace --all-targets -- -D warnings
cargo deny  check                       # license / advisory / source audit
```

CI runs all four on push / PR (`.github/workflows/ci.yml`).

## Contributing

The project is in early bootstrap; expect API churn until v0.2. Issues and
PRs are welcome — please reference `TODO.md` for what's in scope.

## License

MIT — see [`LICENSE`](./LICENSE).
