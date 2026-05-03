---
description: Non-negotiable code and data invariants — error shape, atomic writes, FNV-1a hashing, schema versioning, classification. Violating these silently breaks the FindingsRecord contract.
paths:
  - "crates/cli/src/**/*.rs"
  - "crates/cli/tests/**/*.rs"
---

# Invariants

## R1. `core::Error` variants always carry `path: PathBuf`

Including the catch-all `Io`. Path-less variants force callers to
thread location info themselves, and they always forget. The whole
point of the type is `"<error-text> at {path}"`.

## R2. Use `core::fs::atomic_write` for every mutable state file

Files under this rule:

- `.heal/config.toml`
- `.heal/calibration.toml`
- `.heal/findings/latest.json`
- `.heal/findings/fixed.json`
- `.claude/settings.json`
- Extracted skill files

`std::fs::write` and friends are forbidden for these — SIGINT
mid-write would leave a half-written stub that the next run
deserialises as garbage. The one exception is `regressed.jsonl`
(open-append, line-sized writes are atomic on POSIX within
`PIPE_BUF`).

## R3. Schema-versioned shapes bump on any contract change

`FindingsRecord` is versioned by `FINDINGS_RECORD_VERSION` (currently
`2`). Bump on:

- A field rename.
- A field semantic change (units, sentinel meaning).
- A new `Finding.metric` submetric tag.

`read_latest` peeks the version and silently `Ok(None)`s on mismatch
so the next run rewrites under the new schema. Without a bump, stale
files deserialise into the new struct incorrectly.

When you bump, add a `CHANGELOG.md` "Unreleased" entry with the
migration note.

## R4. `Finding.id` format and stability

```
<metric>:<file>:<symbol-or-*>:<16-hex-fnv1a>
```

The 16-hex digest is FNV-1a 64-bit over `[metric, file, symbol,
content_seed]` chunks (separator `0xff`, see `core::hash`).

The id **must be stable across runs for the same logical finding** —
otherwise `fixed.json` reconciliation breaks. When changing an
observer's seed strategy:

- Prefer structural seeds (function span length, duplicate-block
  signature) over byte-offset seeds, so the id survives reformatting.
- A breaking seed change requires `FINDINGS_RECORD_VERSION` bump.
- New metrics get unique seed prefixes (e.g. `ccn:<span>`,
  `dup:<token_count>:...`). Don't clash.

## R5. `core::hash` (FNV-1a) is the only persistent hasher

Including for `Finding.id`, `config_hash`, duplication's per-token
identity, plugin manifest fingerprint.

`std::hash::DefaultHasher` is officially documented as unstable
across Rust toolchain versions. Switching invalidates every
recorded id after a `rustc` upgrade.

For tuple-shaped inputs use `fnv1a_64_chunked` (separator `0xff`),
not `fnv1a_64(&concat)` — concatenation is collision-prone.

`FxHashMap` / `AHashMap` are fine for **in-memory** maps. Never for
serialized output.

## R6. Cache freshness is `(head_sha, config_hash, worktree_clean)`

`is_fresh_against` is the contract. Dirty worktree → never fresh on
either side. `config_hash` covers `config.toml + calibration.toml`
together.

If a new dimension should invalidate the cache, put it in
`config_hash`'s input. Don't add a fourth tuple element.

## R7. `deny_unknown_fields` everywhere on `Config*`

Every `Config*` and `*Config` struct deserialised from user TOML
derives `#[serde(deny_unknown_fields)]`. Typos surface as
`ConfigInvalid` schema errors at load. Don't relax — the strict mode
prevents "why is my setting being ignored" support burden.

## R8. Per-metric Toggle pattern is symmetric

`Toggle::enabled()` and `Default::default()` must produce the **same**
struct. The pin test
`programmatic_default_matches_serde_default` enforces it. New
`*Config` follows the same pattern.

## R9. `floor_*` overrides go in `config.toml`, not `calibration.toml`

Hand-edits to `calibration.toml` are preserved on read but
overwritten by `heal calibrate --force`. Floors that need to survive
recalibration go in `config.toml` (`[metrics.<m>]` or
`[project.workspaces.metrics.<m>]`).

## R10. `exclude_paths` is gitignore syntax everywhere

Every `exclude_paths` field uses the gitignore DSL: `*`, `**`, `?`,
`[abc]`, `foo/`, `/foo`, `!keep`, `#`. Validated at load.

Bare keywords (`vendor`) match a literal entry named that — usually
the user wants `vendor/` or `*.test.ts`.

## R11. Severity classification lives in `Feature::lower`

Observers emit findings with `severity = Ok`. The Feature pass
classifies (severity assignment) and decorates (hotspot flag).

Don't classify in the observer. Don't bypass `Feature::lower` from
the orchestrator.

## R12. Per-file severity uses `cmp::max`

`SeverityCounts::from_findings` and any per-file rollup take the
worst severity ("worst-finding-wins"). Never replace with weighted
average — that hides Critical findings, which is the signal we surface.

## R13. `BULK_COMMIT_FILE_LIMIT = 50` is load-bearing

ChangeCoupling skips commits with > 50 files (lockfile bumps,
mass-renames). The cost without the cap is quadratic. Don't lower
or remove without a test case showing what new pairs surface.

Note: Churn does **not** apply this cap; only ChangeCoupling does.

## R14. JSON output shapes are stable contracts

Every `--json` output is consumed by skills and CI. A field rename
is a breaking change:

1. `CHANGELOG.md` "Unreleased" `⚠ BREAKING` note.
2. Update consuming skill bodies in the same PR.
3. Bump `FINDINGS_RECORD_VERSION` if applicable.
