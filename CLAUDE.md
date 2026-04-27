# CLAUDE.md

Guidance for Claude Code (and other AI coding agents) when working in this
repository. Read this first; it covers project layout, commands, and the
constraints that shape any change you propose.

## Project at a glance

HEAL is a Rust CLI (binary: `heal`) that turns code-health signals into
work for AI coding agents. The current state is **v0.1 foundation only**:
workspace, config, event-log rotation, and the Claude plugin scaffold exist;
metric observers and the autonomous repair loop do not.

For the *why*, read [`KNOWLEDGE.md`](./KNOWLEDGE.md) (design philosophy,
metric catalog, four-layer architecture, hook policy patterns). For the
*roadmap*, read [`TODO.md`](./TODO.md) — every unchecked item is fair game
for incremental work, but each release tier (v0.1 → v0.5) has a coherent
scope. Don't pull v0.3 work into v0.1.

## Workspace layout

```
crates/
  cli/        # heal-cli  — CLI binary `heal`, command dispatch, plugin embed
  core/       # heal-core — config, eventlog, snapshot, state, paths, error types
  observer/   # heal-observer — metric collectors (skeleton today)
plugins/
  heal/       # Claude Code plugin tree (NOT a Rust crate)
              # Embedded into heal-cli at build time via include_dir!.
              # `heal skills install` materializes it into .claude/plugins/heal/
```

Crate responsibilities are strict:

- **`heal-core`**: pure data types and on-disk formats. No CLI, no agent
  invocation, no observer logic. The library API is what external users
  build against.
- **`heal-observer`**: metric collection only. Stateless. Reads the project
  tree and emits structured payloads. The trait surface is in
  `crates/observer/src/lib.rs`; concrete observers land per TODO item.
- **`heal-cli`**: argument parsing, command dispatch, hook entrypoints,
  plugin extraction. Thin wrapper over `heal-core` and `heal-observer`.

## Toolchain & commands

Rust pinned via [mise](https://mise.jdx.dev) — see `mise.toml`. `cargo` is
either on `PATH` (via mise activation) or at `~/.cargo/bin/cargo`.

```sh
cargo build  --workspace
cargo test   --workspace
cargo fmt    --all
cargo clippy --workspace --all-targets -- -D warnings
cargo deny   check
```

CI (`.github/workflows/ci.yml`) runs all four — keep them green.

## Conventions and invariants

### Error handling
- `heal-core` returns `crate::Result<T>` (alias for `Result<T, heal_core::Error>`).
  All `Error` variants except the catch-all carry a `path: PathBuf` so users
  can locate the failure. Don't add path-less variants.
- `heal-cli` returns `anyhow::Result<()>` from each command and lets `?`
  bridge the two error types.
- `serde_json::to_string` on owned structs (`Snapshot`, `State`) is
  treated as infallible — use `.expect("… serialization is infallible")`
  rather than propagating an unreachable error.

### Configuration (`.heal/config.toml`)
- All config structs use `#[serde(deny_unknown_fields)]`. Typos in user
  configs surface as schema errors instead of silently dropping settings.
  Don't relax this — better to require explicit migration.
- Metric defaults asymmetry: serde-side default (when section is absent
  from TOML) is **enabled**; programmatic `Default` is also **enabled**.
  Both paths must produce the same struct; there's a test that pins this.
- The `Toggle` trait + `default_enabled` glue lets serde missing-section
  defaults vary per metric. If you add a new metric, follow the existing
  pattern (`*Config` struct + `Default` + `Toggle` + register on
  `MetricsConfig`).

### Event log (`.heal/snapshots/YYYY-MM.jsonl` and `.heal/logs/YYYY-MM.jsonl`)
- Both directories use the same generic `heal_core::eventlog::EventLog`
  store: append-only, month-rotated, reads transparent across `.gz`.
  Compaction ships in v0.2+.
- **`snapshots/`** holds `MetricsSnapshot` events written by the `commit`
  hook. `heal status` reads these for the metric series and delta. Decode
  the latest record with `snapshot::MetricsSnapshot::latest_in(&log)`.
- **`logs/`** holds raw hook events (`commit` / `edit` / `stop`). The
  `commit` entry carries lightweight `CommitInfo` metadata only (sha,
  parent, author email, message summary, files_changed/insertions/
  deletions); the heavy metric payload stays in `snapshots/`. `heal logs`
  reads these.
- `EventLog::iter_segments(segments)` exists so callers that already paid
  for `segments()` (e.g. `heal status`) don't re-glob the directory. Use
  it.
- Don't introduce a different rotation strategy without updating
  `KNOWLEDGE.md` § 11.2.

### Claude plugin
- Source of truth lives at `plugins/heal/`. **Do not** copy plugin assets
  into a Rust crate.
- `heal-cli` embeds the directory at build time via
  `include_dir!("$CARGO_MANIFEST_DIR/../../plugins/heal")`.
- `heal skills install` extracts the embedded tree to
  `.claude/plugins/heal/` and chmods `*.sh` to `0755` on Unix.

### Lints
- `clippy::pedantic = warn` at the workspace level, plus `-D warnings`
  in CI. New code must pass clippy clean. If a lint is genuinely
  inappropriate, prefer a localized `#[allow(clippy::<lint>)]` with a
  comment explaining why over disabling the lint workspace-wide.

## v0.1 scope guardrails

When proposing changes, check the unchecked TODO items in `TODO.md`:

- ✅ **In scope for v0.1**: tokei integration, tree-sitter for TS/JS,
  CCN / Cognitive / Duplication / Churn / Change-Coupling observers,
  `heal status` finding output, `heal check` Claude wiring, hook scripts,
  event-log rotation polish, install/update flow.
- 🚫 **Out of scope**: `heal run` (v0.2), additional languages beyond
  TS/JS (v0.2+), LSP-based metrics (v0.5), Doc generation (v0.5),
  multi-agent abstraction (v0.4).

Stub commands like `commands/check.rs` intentionally return `Result<()>`
to keep the dispatcher signature uniform. The `#[allow(clippy::unnecessary_wraps)]`
there is intentional — don't remove it until the stub becomes real.

## Documentation pointers

- [`KNOWLEDGE.md`](./KNOWLEDGE.md) — full design doc. § 11 covers the
  HEAL-specific implementation plan.
- [`TODO.md`](./TODO.md) — release roadmap with checked / unchecked items.
- [`README.md`](./README.md) — user-facing entry point. Keep it accurate
  whenever public-facing behavior changes.

## When in doubt

1. Re-read the relevant section of `KNOWLEDGE.md`.
2. Check whether the change is in v0.1 scope per `TODO.md`.
3. Ask in the PR / issue thread before introducing a new crate, a new
   external dependency, or a schema change to `.heal/`.
