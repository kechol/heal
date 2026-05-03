# CLAUDE.md

Guidance for Claude Code (and other AI coding agents) when working in
this repository. Read this first; it covers project layout, commands,
and the constraints that shape any change you propose.

## Project at a glance

HEAL is a Rust CLI (binary: `heal`) that turns code-health signals into
work for AI coding agents. v0.1 ships the **observe** half of the loop:
six metric observers (LOC, CCN, Cognitive, Churn, Change Coupling,
Duplication, Hotspot composition), the post-commit git hook and
Claude Code's `settings.json` hook commands that drive them, and
`heal status` / `heal metrics` / `heal diff`
for surfacing findings. `heal status` runs the analyzer, classifies by
Severity, and writes `.heal/checks/`; `heal checks` (top-level
browser) and `heal diff <git-ref>` read it. The autonomous repair loop
(`heal run`, PR generation) lands in v0.2+.

For the user-facing overview see [`README.md`](./README.md).

## Workspace layout

```
crates/
  cli/                       # heal-cli — the only Rust crate; published to crates.io
    src/
      main.rs                # thin entrypoint: parse + dispatch
      lib.rs                 # internal pub modules — exposed only so tests/ can reach them
      cli.rs                 # clap definitions
      commands/              # one file per subcommand
      core/                  # config, eventlog, snapshot, state, paths, error types
      observer/              # LOC, complexity (CCN/Cognitive), churn, coupling,
                             # duplication, hotspot composition
    plugins/heal/skills/     # Claude Code skills tree, embedded via include_dir!
    queries/                 # tree-sitter queries (rust/, typescript/) read via include_str!
plugins → crates/cli/plugins # top-level convenience symlink
```

The workspace is intentionally a single crate. The original three-crate
split (`heal-core` / `heal-observer` / `heal-cli`) was inlined so
`cargo install heal-cli` is the one supported install path; internal
modules are not separately published to crates.io. Module shape is
preserved (`crate::core::*`, `crate::observer::*`) so call sites read the
same as before. The `lib.rs` is `#[doc(hidden)]` and treated as
unstable internal API — the public contract is the `heal` CLI surface
documented in `README.md`.

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
- `crate::core` returns `core::Result<T>` (alias for
  `Result<T, crate::core::Error>`). All `Error` variants except the
  catch-all carry a `path: PathBuf` so users can locate the failure.
  Don't add path-less variants.
- Top-level commands return `anyhow::Result<()>` and let `?` bridge the
  two error types via `From<core::Error> for anyhow::Error`.
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

### No persistent metrics history
- The `commit` hook does not write to `.heal/snapshots/` (or anywhere
  else). It runs the observers, classifies against the calibration on
  disk, and emits a one-line nudge — that's it.
- `heal metrics` recomputes everything on every invocation. There is no
  delta vs. a previous snapshot. The motivation: per-team determinism
  beats trend tracking. With the cache and snapshots gone, every
  teammate on the same commit + same `config.toml` + same
  `calibration.toml` sees identical findings.
- `heal hook edit` / `heal hook stop` are kept as no-op CLI variants
  for back-compat with stale `settings.json` registrations; Phase E
  retires the registration so they stop firing.

### Result cache (`.heal/checks/`)
- `heal status` is the **only writer** of `.heal/checks/<segment>.jsonl`
  and `latest.json`. `heal checks` and `heal diff` are pure readers.
  `heal mark-fixed` is a second writer scoped to `fixed.jsonl`.
  The cache models a TODO list — every Finding has a deterministic id
  (`<metric>:<file>:<symbol>:<fnv1a>`) so an unfixed problem reappears
  under the same id on the next run.
- `checks/YYYY-MM.jsonl` (append-only) plus three side files:
  `latest.json` (atomic mirror of the most recent record),
  `fixed.jsonl` (skill committed a fix), `regressed.jsonl` (a fix
  was re-detected). `core::check_cache` owns the schema.
- Idempotency: `heal status` short-circuits when
  `(head_sha, config_hash, worktree_clean=true)` matches the latest
  cached record — re-running on the same commit is free. Dirty
  worktrees never count as fresh.
- `config_hash` covers `config.toml + calibration.toml`. A
  `heal calibrate` invalidates every cache row, which is correct: the
  Severity ladder shifted under us.
- `reconcile_fixed` walks `fixed.jsonl` against new findings on every
  fresh run. Re-detected entries move to `regressed.jsonl` and the
  renderer surfaces them. Don't add a way to suppress this — the
  warning is the whole point of tracking fixes separately.

### Hashing
- Persistent hashes (duplication's per-token identity, the plugin
  fingerprint manifest) use a hand-rolled FNV-1a 64-bit so output is
  stable across processes and Rust toolchain versions. Do not switch to
  `std::hash::DefaultHasher` — its algorithm is explicitly unstable
  across releases, which would invalidate every recorded fingerprint
  after a `rustc` upgrade.

### Claude skills
- Source of truth lives at `crates/cli/plugins/heal/skills/`. Each
  top-level child is a self-contained skill directory. The tree sits
  inside the `heal-cli` crate directory so `cargo publish` includes it
  in the published tarball — `include_dir!` is a compile-time read and
  Cargo only packages files inside the crate dir.
- `heal-cli` embeds the directory at build time via
  `include_dir!("$CARGO_MANIFEST_DIR/plugins/heal/skills")`.
- `heal skills install` extracts the embedded tree to
  `<project>/.claude/skills/<skill-name>/` (no marketplace, no plugin
  wrapper — Claude Code natively discovers project-scope skills under
  `.claude/skills/`). Each extracted file's fingerprint is recorded in
  `.heal/skills-install.json` for drift detection on
  `heal skills update`.
- The same install merges HEAL's hook commands into
  `<project>/.claude/settings.json`: `PostToolUse → "heal hook edit"`,
  `Stop → "heal hook stop"`. Merge is additive — existing user hook
  blocks are preserved; uninstall removes only HEAL's command lines.
- `heal hook` is robust against missing `.heal/`: if a project never
  ran `heal init`, the hook silently no-ops so it doesn't pollute an
  un-opted-in worktree.

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
