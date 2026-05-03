# Skills and hooks

Internal contract for the Claude Code integration. The user-facing
description lives in `docs/claude-skills.md` (Starlight); this is the
implementation reference.

---

## Big picture

HEAL ships four skills bundled inside the `heal-cli` binary. They are
extracted to `.claude/skills/<skill>/` on `heal init` (when accepted)
and on `heal skills install`. Claude Code natively discovers
project-scope skills under `.claude/skills/` — no marketplace, no
plugin wrapper.

HEAL **does not register any Claude Code hooks** as of v0.2+. The only
hook that exists is the post-commit **git** hook installed by
`heal init`. The presence of `claude_settings::register` is purely for
sweeping legacy entries that older HEAL versions wrote.

---

## Bundled skill tree

Source: `crates/cli/skills/`. The path is **inside the crate dir** so
`cargo publish` includes the tree in the tarball.

```
crates/cli/skills/
├── heal-cli/                    # CLI reference (read-only)
│   └── SKILL.md
├── heal-config/                 # one-shot calibrate + write config
│   ├── SKILL.md
│   └── references/
│       └── config.md
├── heal-code-review/            # read-only architectural review
│   ├── SKILL.md
│   └── references/
│       ├── architecture.md
│       ├── metrics.md
│       └── readability.md
└── heal-code-patch/             # mechanical drain, one finding/commit
    └── SKILL.md
```

### Per-skill role

| Skill | Role | Pair |
|---|---|---|
| `heal-cli` | CLI contract reference; load before shelling out to `heal`. | — |
| `heal-config` | One-shot: calibrate + write `.heal/config.toml` tuned to a strictness level (Strict / Default / Lenient) chosen via `AskUserQuestion`. Read-only on the codebase. | — |
| `heal-code-review` | Read every `heal status --all --json` finding, deeply investigate, return one architectural reading + prioritised refactor TODO list. Read-only — proposes only. | write counterpart `heal-code-patch` |
| `heal-code-patch` | Drain the cache fixing one finding per commit in Severity order. Refuses dirty worktree. Calls `heal mark-fixed` after each commit. **Does not push or open PRs.** | write counterpart of `heal-code-review` |

The pair `heal-code-review` ↔ `heal-code-patch` is intentional: review
includes architecture-level proposals (DDD, layering, module-depth);
patch is mechanical refactor only.

---

## Embedding (`skill_assets.rs`)

```rust
pub static SKILLS_DIR: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/skills");

pub const SKILLS_DEST_REL: &str = ".claude/skills";
```

Skills are read at compile time. Runtime extraction does no FS reads of
the bundled tree — `Dir<'_>::contents()` returns embedded byte slices.

### Extract modes

```rust
pub enum ExtractMode {
    InstallSafe,                  // default install: skip existing
    InstallForce,                 // install --force: overwrite all
    Update { force: bool },       // update: drift-aware
}
```

| Caller | Mode |
|---|---|
| `heal skills install` | `InstallSafe` |
| `heal skills install --force` | `InstallForce` |
| `heal skills update` | `Update { force: false }` |
| `heal skills update --force` | `Update { force: true }` |
| `heal init` (yes path) | `InstallSafe` |

### Drift detection (no sidecar manifest)

```rust
fn user_modified(file: &File<'_>, rel_path: &Path, on_disk: &[u8]) -> bool {
    canonical_user_bytes(rel_path, on_disk) != file.contents()
}
```

`canonical_user_bytes` strips the `metadata:` block from SKILL.md;
other files are returned verbatim. Drift is therefore a pure function
of `(on-disk, bundled)` bytes — same on every machine, no state file.

The retired `skills-install.json` was machine-local and untracked, so
drift verdicts diverged across teammates re-installing. Removed in
commit `89d849a` (`refactor!(skills): drop skills-install.json, derive
drift from frontmatter`). **Don't reintroduce it.**

### SKILL.md `metadata:` block

Extract injects this block into every SKILL.md frontmatter:

```yaml
metadata:
  heal-version: <env!("CARGO_PKG_VERSION") at compile time>
  heal-source: bundled
```

Idempotence: on every extract, the existing block is stripped first
(via `strip_skill_metadata`), then re-injected. Multiple extracts
produce identical bytes — tested by
`skill_md_install_carries_frontmatter_metadata` and
`extract_update_unchanged_when_only_metadata_was_stripped`.

The block must **not** be hand-authored in source. Edit the rest of
the frontmatter; the metadata block is auto-managed.

### `ExtractStats`

```rust
pub struct ExtractStats {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
    pub skipped: Vec<String>,           // InstallSafe: pre-existing
    pub user_modified: Vec<String>,     // Update: drift, force not set
}
```

Surfaced in `heal skills` JSON output for skill-driven flows.

---

## `.claude/settings.json` reconciliation (`claude_settings.rs`)

HEAL **only** reads/writes `.claude/settings.json` to:

1. Sweep legacy `heal hook edit` / `heal hook stop` entries from any
   `hooks.<event>[].hooks[].command`.
2. Sweep pre-v0.2 marketplace artifacts.

The current install path **adds nothing** to this file — that's
deliberate.

### Legacy command sweep

```rust
const LEGACY_HEAL_COMMANDS: &[&str] = &["heal hook edit", "heal hook stop"];
```

Walk: `hooks[<event>][].hooks[].command == "heal hook edit"` (or stop)
→ drop the inner-hook entry → drop the block if empty → drop the event
array if empty → drop the `hooks` object if empty. Atomic write via
`core::fs::atomic_write`.

### Pre-v0.2 marketplace sweep (uninstall only)

| Artifact | Action |
|---|---|
| `.claude/plugins/heal` | `remove_dir_all` |
| `.claude-plugin/marketplace.json` | `remove_file` |
| `.claude-plugin/` | best-effort `remove_dir` if empty |
| `extraKnownMarketplaces["heal-local"]` in settings | drop key |
| `enabledPlugins["heal@heal-local"]` in settings | drop key |
| Empty parent objects after cleanup | drop |

These shapes existed in the very early plugin-distribution attempt;
keep the sweep as-is so users on stale layouts can clean up via `heal
skills uninstall`.

### Outcome enum

```rust
pub enum WriteAction { Created, Updated, Unchanged }
```

`Unchanged` means the file's bytes already matched the post-sweep state.

### Public API summary

| Function | Use case | Caller |
|---|---|---|
| `wire(project)` | sweep legacy + report | `heal init`, `heal skills install` |
| `register(project)` | sweep legacy commands only (idempotent) | inner of `wire` |
| `unregister(project)` | full cleanup (commands + marketplace artifacts) | `heal skills uninstall` |

---

## Post-commit git hook

Installed by `heal init` to `.git/hooks/post-commit` with marker
`# heal post-commit hook`. The marker is what lets `heal init` refresh
the script idempotently without clobbering a user-authored hook.

```sh
#!/usr/bin/env sh
# heal post-commit hook
if command -v heal >/dev/null 2>&1; then
  heal hook commit || true
fi
exit 0
```

Design:

- Failures swallowed (`|| true`) → never blocks a commit.
- Skips entirely if `heal` is not on `PATH`.
- `heal hook commit` itself silently no-ops if `.heal/` doesn't exist
  (uninitialized-project safety).

The post-commit hook is the **only** hook HEAL installs. Don't add
SessionStart, Stop, PostToolUse, or any other Claude Code event.

---

## Test helpers (`test_support.rs`)

Used by `init::tests::*` and other hook tests:

- `git_bin()` — cached `OnceLock` PATH lookup with fallbacks
  (`/usr/bin/git`, `/usr/local/bin/git`, `/opt/homebrew/bin/git`).
  Cargo runs tests in parallel; tests mutate `PATH` to drive `claude`
  lookup logic, so caching git's location avoids races.
- `git(cwd, args)` — shell out, panic on nonzero.
- `init_repo(cwd)` — `git init -q` + `commit.gpgsign=false`.
- `commit(cwd, file, body, email, msg)` — write + add + commit.

---

## Skill body conventions

Each `SKILL.md` follows:

1. YAML frontmatter:
   - `name` (matches directory name).
   - `description` — long, trigger-rich, ends with the slash-command
     form (e.g. `"/heal-cli"`).
   - `metadata:` (auto-injected on extract — don't hand-edit).
2. Body: a single document the agent reads top-to-bottom. References
   in `references/` are loaded **on demand** to keep the initial token
   budget small.

Trigger-rich descriptions: trigger phrases like `"how do I run heal"`,
`"fix the heal findings"`, `"review the codebase health"` are spelled
out so Claude Code's skill-matching can pick them up. When you add a
new skill, follow this pattern.

---

## What you must **not** do

- Don't reintroduce `skills-install.json` or any sidecar manifest.
- Don't reintroduce `marketplace.json` or `.claude/plugins/heal/`.
- Don't add new entries to Claude Code's `settings.json` — HEAL is
  hooks-free now.
- Don't hand-author `metadata:` blocks in source SKILL.md files.
- Don't add a SessionStart, Stop, PostToolUse, or Edit hook.
- Don't move skill source out of `crates/cli/` — `cargo publish`
  only packages files inside the crate dir.
- Don't merge `heal-code-review` and `heal-code-patch`. The split is
  intentional (review = read, patch = write).
