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
for surfacing findings. `heal status` runs the analyzer, classifies by Severity, writes the
result to `.heal/findings/`, and renders it. `heal diff <git-ref>`
reads the same cache to compare against the live worktree. The
autonomous repair loop (`heal run`, PR generation) lands in v0.2+.

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
      core/                  # config, calibration, finding, findings_cache,
                             # severity, term, paths, fs, hash, monorepo, error
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

## Domain glossary

Use the **canonical** column in new code, comments, and docs. The
public JSON shape (`.heal/findings/latest.json`) is part of this
contract — bumping `FINDINGS_RECORD_VERSION` is the prescribed escape
hatch when a field rename is unavoidable.

Most names are self-evident from `crate::core::*` and the
`.heal/findings/` layout — only the conventions you'd otherwise have
to derive are listed here.

| Concept                               | Canonical                                                           | Notes                                                       |
|---------------------------------------|---------------------------------------------------------------------|-------------------------------------------------------------|
| Result of one `heal status` run       | `FindingsRecord`                                                    | `.heal/findings/latest.json`. Schema-versioned via `FINDINGS_RECORD_VERSION` (currently `2`); `read_latest` peeks at the version field and returns `Ok(None)` on any mismatch so the next run silently rewrites under the new schema. |
| Severity ladder                       | `Severity` { `Ok`, `Medium`, `High`, `Critical` }                   | Per-file aggregation uses `cmp::max` (worst-finding-wins).  |
| Calibration pair                      | `Calibration` (per-metric percentile breaks) + `HotspotCalibration` (per-file composite) | Both live in `core::calibration`.                |
| Re-detected-fix cross-ref             | `RegressedEntry::regressed_in_record_id`                            | Points at the `FindingsRecord.id` (ULID, chronological) that re-detected the finding — the only field that links the append-only `regressed.jsonl` back to a specific run. |
| Live subcommands                      | `init`, `hook`, `metrics`, `status`, `diff`, `mark-fixed`, `skills`, `calibrate` | `checks`, `compact`, `logs`, `snapshots`, and the entire `fix` group were removed. |

## Conventions and invariants

### Error handling
- `crate::core` returns `core::Result<T>` (alias for
  `Result<T, crate::core::Error>`). All `Error` variants except the
  catch-all carry a `path: PathBuf` so users can locate the failure.
  Don't add path-less variants.
- Top-level commands return `anyhow::Result<()>` and let `?` bridge the
  two error types via `From<core::Error> for anyhow::Error`.
- `serde_json::to_string` on owned structs we control (e.g.
  `FindingsRecord`, `FixedMap`) is treated as infallible — use
  `.expect("… serialization is infallible")` rather than propagating
  an unreachable error.

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
  for back-compat with stale `settings.json` registrations; new installs
  no longer add them (`heal skills install` actively sweeps them out).

### Result cache (`.heal/findings/`)
- `heal init` writes a project-level `.heal/.gitignore` listing
  `findings/` so volatile state stays out of git history. `config.toml`
  and `calibration.toml` remain tracked so teammates share the same
  Severity ladder.
- `heal status` is the **only writer** of `latest.json`. `heal diff`
  is a pure reader. `heal mark-fixed` is a second writer scoped to
  `fixed.json`. The cache models a TODO list — every Finding has a
  deterministic id (`<metric>:<file>:<symbol>:<fnv1a>`) so an unfixed
  problem reappears under the same id on the next run.
- The cache is **single-record by design**. Three files live under
  `.heal/findings/`: `latest.json` (the current `FindingsRecord`),
  `fixed.json` (`BTreeMap<finding_id, FixedFinding>` — bounded by
  outstanding fix claims, never appended), and `regressed.jsonl` (the
  one append-only audit trail of "a previously-fixed finding was
  re-detected"). No YYYY-MM.jsonl history is written.
- Idempotency: `heal status` short-circuits when
  `(head_sha, config_hash, worktree_clean=true)` matches the cached
  record — re-running on the same commit is free. Dirty worktrees
  never count as fresh.
- `config_hash` covers `config.toml + calibration.toml`. A
  `heal calibrate` shifts the hash, so the next `heal status` rebuilds
  rather than reading a stale cache.
- `reconcile_fixed` walks `fixed.json` against new findings on every
  fresh run. Re-detected entries are removed from the map and recorded
  in `regressed.jsonl` so the renderer can warn the user. Don't add a
  way to suppress this — the warning is the whole point of tracking
  fixes separately.

### `heal diff` worktree mode
- `heal diff <git-ref>` first tries the cache (`latest.json` whose
  `head_sha` matches the resolved ref). On a miss it materialises the
  source at the ref via `git worktree add --detach <tempdir> <sha>`,
  runs the observer pipeline against the worktree using the *current*
  `config.toml`/`calibration.toml`, and tears the worktree down via a
  `Drop` guard so a `?` short-circuit can't leak `.git/worktrees/`.
  The "from" record's findings reflect today's rules applied to the
  historical source — not the historical findings the user might have
  seen at the time. That's deliberate (apples-to-apples).
- The worktree path is gated by `[diff].max_loc_threshold` (default
  `200_000` LOC, override in `config.toml`). Over the limit, `heal
  diff` exits with code `2` and prints the manual two-branch recipe.
  The LOC count uses `LocObserver::scan` on the *current* worktree as
  a proxy — repos rarely shift LOC by orders of magnitude between
  commits, so this is good enough as a cost gate without paying for a
  second scan.

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
  `.claude/skills/`). There is no sidecar manifest: each SKILL.md is
  stamped with a `metadata:` block in its YAML frontmatter
  (`heal-version`, `heal-source`), and `heal skills update` derives
  drift by comparing `canonical(on-disk)` (the metadata block stripped)
  against the bundled raw bytes. Same machine or different teammate's
  machine — the drift verdict is the same function of the on-disk + 
  bundled bytes.
- HEAL no longer registers any Claude Code hooks. `heal skills install`
  / `heal init` only sweep legacy `heal hook edit` / `heal hook stop`
  entries left over from earlier installs (and the pre-v0.2 marketplace
  plugin tree if present). User hook entries are preserved.
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
