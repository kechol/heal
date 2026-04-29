# CLAUDE.md

Guidance for Claude Code (and other AI coding agents) when working in
this repository. Read this first; it covers project layout, commands,
and the constraints that shape any change you propose.

## Project at a glance

HEAL is a Rust CLI (binary: `heal`) that turns code-health signals into
work for AI coding agents. v0.1 ships the **observe** half of the loop:
six metric observers (LOC, CCN, Cognitive, Churn, Change Coupling,
Duplication, Hotspot composition), the post-commit and Claude plugin
hooks that drive them, and `heal status` / `heal check` for surfacing
findings. The autonomous repair loop (`heal run`, PR generation) lands
in v0.2+.

For the user-facing overview see [`README.md`](./README.md).

## Workspace layout

```
crates/
  cli/        # heal-cli  — CLI binary `heal`, command dispatch, plugin embed
  core/       # heal-core — config, eventlog, snapshot, state, paths, error types
  observer/   # heal-observer — metric collectors
plugins/
  heal/       # Claude Code plugin tree (NOT a Rust crate)
              # Embedded into heal-cli at build time via include_dir!.
              # `heal skills install` materialises it into .claude/plugins/heal/
```

Crate responsibilities are strict:

- **`heal-core`**: pure data types and on-disk formats. No CLI, no agent
  invocation, no observer logic. The library API is what external users
  build against.
- **`heal-observer`**: metric collection only. Stateless. Reads the
  project tree and emits structured payloads. The trait surface is in
  `crates/observer/src/lib.rs`.
- **`heal-cli`**: argument parsing, command dispatch, hook entrypoints,
  plugin extraction. Thin wrapper over `heal-core` and `heal-observer`.

## Toolchain & commands

Rust pinned via [mise](https://mise.jdx.dev) — see `mise.toml`. `cargo`
is either on `PATH` (via mise activation) or at `~/.cargo/bin/cargo`.

```sh
cargo build  --workspace
cargo test   --workspace
cargo fmt    --all
cargo clippy --workspace --all-targets -- -D warnings
cargo deny   check
```

CI (`.github/workflows/ci.yml`) runs all five — keep them green.

## Conventions and invariants

### Error handling
- `heal-core` returns `crate::Result<T>` (alias for
  `Result<T, heal_core::Error>`). All `Error` variants except the
  catch-all carry a `path: PathBuf` so users can locate the failure.
  Don't add path-less variants.
- `heal-cli` returns `anyhow::Result<()>` from each command and lets `?`
  bridge the two error types.
- `serde_json::to_string` on owned structs (`Snapshot`, `State`) is
  treated as infallible — use `.expect("… serialization is infallible")`
  rather than propagating an unreachable error.

### Configuration (`.heal/config.toml`)
- All config structs use `#[serde(deny_unknown_fields)]`. Typos in user
  configs surface as schema errors instead of silently dropping
  settings. Don't relax this — better to require explicit migration.
- Metric defaults asymmetry: serde-side default (when section is absent
  from TOML) is **enabled**; programmatic `Default` is also **enabled**.
  Both paths must produce the same struct; there's a test
  (`programmatic_default_matches_serde_default`) that pins this.
- The `Toggle` trait + `default_enabled` glue lets serde missing-section
  defaults vary per metric. If you add a new metric, follow the existing
  pattern (`*Config` struct + `Default` + `Toggle` + register on
  `MetricsConfig`).

### Event log (`.heal/snapshots/YYYY-MM.jsonl` and `.heal/logs/YYYY-MM.jsonl`)
- Both directories use the same generic `heal_core::eventlog::EventLog`
  store: append-only, month-rotated, reads transparent across `.gz`.
  Compaction ships in v0.2+.
- **`snapshots/`** holds `MetricsSnapshot` events written by the
  `commit` hook. `heal status` reads these for the metric series and
  delta. Decode the latest record with
  `snapshot::MetricsSnapshot::latest_in(&log)`. Records that fail to
  decode (legacy payloads, mid-write truncation) are skipped silently —
  do not change `latest_in_segments` to propagate parse errors.
- **`logs/`** holds raw hook events (`commit` / `edit` / `stop` /
  `session-start`). The `commit` entry carries lightweight `CommitInfo`
  metadata only (sha, parent, author email, message summary,
  files_changed/insertions/deletions); the heavy metric payload stays
  in `snapshots/`. `heal logs` reads these.
- `EventLog::iter_segments(segments)` exists so callers that already
  paid for `segments()` (e.g. `heal status`) don't re-glob the
  directory. Use it.

### Runtime state (`.heal/state.json`)
- Holds `last_fired` (per-rule cool-down timestamps for the SessionStart
  nudge) and the placeholder `open_proposals` (used in v0.2 once
  `policy.action = execute` lands).
- `State::save` writes via temp-file + rename so a SIGINT mid-write
  never leaves a half-written file. `State::load` falls back to defaults
  on `NotFound` so a freshly initialised project still works.
- The struct deliberately does **not** use `deny_unknown_fields` so a
  newer binary's additions don't break an older binary's reads.

### Hashing
- Persistent hashes (duplication's per-token identity, the plugin
  fingerprint manifest) use a hand-rolled FNV-1a 64-bit so output is
  stable across processes and Rust toolchain versions. Do not switch to
  `std::hash::DefaultHasher` — its algorithm is explicitly unstable
  across releases, which would invalidate every recorded fingerprint
  after a `rustc` upgrade.

### Claude plugin
- Source of truth lives at `plugins/heal/`. **Do not** copy plugin
  assets into a Rust crate.
- `heal-cli` embeds the directory at build time via
  `include_dir!("$CARGO_MANIFEST_DIR/../../plugins/heal")`.
- `heal skills install` extracts the embedded tree to
  `.claude/plugins/heal/` and chmods `*.sh` to `0755` on Unix. Each
  extracted file's fingerprint is recorded in `.heal-install.json` for
  drift detection on `heal skills update`.

### Lints
- `clippy::pedantic = warn` at the workspace level, plus `-D warnings`
  in CI. New code must pass clippy clean. If a lint is genuinely
  inappropriate, prefer a localized `#[allow(clippy::<lint>)]` with a
  comment explaining why over disabling the lint workspace-wide.

## Scope guardrails

When proposing changes, keep these v0.1 boundaries in mind:

- ✅ **In scope**: bug fixes, observer accuracy, CLI ergonomics, plugin
  asset polish, additional regression tests, doc-correctness fixes.
- 🚫 **Out of scope**: `heal run` (PR generation), additional language
  parsers beyond TS/JS/Rust, LSP-based metrics, doc-skew / doc-coverage
  observers, multi-agent provider abstraction. These are deferred.

## When in doubt

1. Read the relevant source — file-level doc comments are kept current
   and usually answer "why is it shaped this way".
2. Check tests for the contract: `crates/*/tests/` exercises every
   public API path that matters.
3. Open an issue before introducing a new crate, a new external
   dependency, or a schema change to `.heal/`.
