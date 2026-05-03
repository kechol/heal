# Conventions

Workspace-wide conventions that don't fit one of the layered docs.

For per-area facts:

- Error handling, atomic writes, hashing, schema invariants → `data-model.md`.
- Severity classification, hotspot decoration → `architecture.md` and `observers.md`.
- JSON output, pager, exit codes → `commands.md`.

---

## Idempotency at the command level

- `heal init` rewrites `.heal/.gitignore`, `config.toml`, and the
  post-commit hook only when content differs. `--force` overwrites.
- `heal status` short-circuits when the cache is fresh
  (`(head_sha, config_hash, worktree_clean=true)` match — see
  `data-model.md`).
- `heal hook commit` short-circuits silently if `.heal/` doesn't exist.
- `heal skills install` skips existing files in `InstallSafe` mode;
  `--force` overwrites.

Dirty worktrees are never considered fresh.

---

## Lints and toolchain

- Workspace-wide `clippy::pedantic = warn`, plus `-D warnings` in CI.
- Don't suppress workspace-wide. Localised `#[allow(clippy::<lint>)]`
  with a one-line comment is the right level.
- Toolchain pinned via mise (`mise.toml`). Bump in its own PR.
- `cargo deny check` runs in CI; new deps may need a `deny.toml`
  exception (prefer dropping the dep over adding an exception).

CI runs five gates: `cargo build --workspace`, `cargo test --workspace`,
`cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo deny check`.

---

## Tests

- Unit tests in `#[cfg(test)] mod tests` next to the code.
- Integration tests under `crates/cli/tests/`, one file per module
  (`core_config.rs`, `observer_loc.rs`, `observer_complexity.rs`,
  etc.).
- Shared helpers: `crates/cli/tests/common/mod.rs` and
  `crates/cli/src/test_support.rs`.
- Tests that touch git **must** go through `test_support::git_bin()`
  (cached `OnceLock`) — cargo runs tests in parallel and tests mutate
  `PATH` to drive `claude` lookup logic, so direct `git` calls race.
- Tests that need a working tree use `test_support::{init_repo,
  commit}` — never assume host git config (gpgsign etc. is disabled
  in `init_repo`).

JSON-output tests assert on **specific field names** to pin the
contract:

```rust
let parsed: serde_json::Value = serde_json::from_str(&out)?;
assert_eq!(parsed["version"], 2);
assert!(parsed["findings"][0]["id"].is_string());
```

---

## Workspace path conventions

`[[project.workspaces]] path` is **project-relative, no leading `/`**.
Workspace `exclude_paths` are workspace-relative; HEAL re-anchors them
when feeding to `ExcludeMatcher` (see CHANGELOG "Workspace
`exclude_paths` is wired").

---

## Documentation co-update

When touching:

| If you change… | Co-update… |
|---|---|
| user-visible CLI flag or output | `docs/cli.md`, `docs/quick-start.mdx`, `README.md` |
| metric definition | `docs/metrics.md`, `crates/cli/skills/heal-code-review/references/metrics.md` |
| `.heal/config.toml` schema | `docs/configuration.md`, `crates/cli/skills/heal-config/references/config.md` |
| `FindingsRecord` JSON | bump `FINDINGS_RECORD_VERSION`, update `data-model.md`, add `CHANGELOG.md` "Unreleased" entry |
| canonical term in `glossary.md` | sweep across source, tests, skill bodies, Starlight (en + ja), `README.md`, `CLAUDE.md` |

Docs are part of the code. PRs that ship without doc co-update are
not ready.

---

## Japanese / English

| Surface | Language |
|---|---|
| `README.md`, `CHANGELOG.md`, `docs/src/content/docs/*` | English |
| `docs/src/content/docs/ja/*` | Japanese (mirror) |
| `.claude/docs/`, `.claude/rules/`, `CLAUDE.md` | English |
| Source comments | English |

When updating Japanese pages, watch for unnatural spaces around CJK
characters — common mechanical-translation artifact.

---

## Comments

Comments explain **why**. The code already says **what**.

- No "increment counter" / "check if file exists" / "loop over items".
- Yes "First parent only — merge commits inflate churn otherwise"
  / "FNV-1a hand-rolled because DefaultHasher is unstable across rustc".
- No "Added to fix #123" / "Used by skill X" — comments rot when the
  link disappears. Put that in the commit message and PR body.

No emoji in source files. Tier labels in renderer output (T0 Must
🎯, T1 Should 🟡, Advisory ℹ️) are the exception — those are the
shipping output.
