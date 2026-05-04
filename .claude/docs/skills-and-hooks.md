# Skills and hooks

Internal contract for the Claude Code integration. The user-facing
description lives in `docs/claude-skills.md` (Starlight); this is the
implementation reference.

---

## Big picture

HEAL ships ten skills bundled inside the `heal-cli` binary. They are
extracted to `.claude/skills/<skill>/` on `heal init` (when accepted)
and on `heal skills install`. Claude Code natively discovers
project-scope skills under `.claude/skills/` — no marketplace, no
plugin wrapper.

Skills group along the three feature families:

- **Code** (always-on observer family): `heal-cli`, `heal-setup`,
  `heal-code-review`, `heal-code-patch`.
- **`[features.docs]`** (opt-in): `heal-doc-pair-setup`,
  `heal-doc-scaffold`, `heal-doc-review`, `heal-doc-patch`.
- **`[features.test]`** (opt-in): `heal-test-reporter-setup`,
  `heal-test-review`, `heal-test-patch`.

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
├── heal-cli/                       # CLI reference (read-only)
├── heal-setup/                    # one-shot calibrate + write config
├── heal-code-review/               # read-only architectural review
├── heal-code-patch/                # mechanical drain, one finding/commit
├── heal-doc-pair-setup/            # write .heal/doc_pairs.json (SSoT)
├── heal-doc-scaffold/              # stand up the wiki tree from scratch
├── heal-doc-review/                # read-only Diátaxis-grounded doc review
├── heal-doc-patch/                 # mechanical drain of doc findings
├── heal-test-reporter-setup/       # propose lcov reporter + CI config
├── heal-test-review/               # read-only test-pyramid review
└── heal-test-patch/                # mechanical drain of test findings
```

Each skill has at least a `SKILL.md`; some carry `references/` files
loaded on demand (`heal-setup/references/config.md`,
`heal-code-review/references/{architecture,metrics,readability}.md`,
`heal-doc-pair-setup/references/doc-pairs-schema.md`, etc.).

### Per-skill role

| Skill | Family | Role | Pair |
|---|---|---|---|
| `heal-cli` | Code | CLI contract reference; load before shelling out to `heal`. | — |
| `heal-setup` | Code | One-shot setup wizard: calibrate + write `.heal/config.toml` tuned to a strictness level (Strict / Default / Lenient) chosen via `AskUserQuestion`, then gate `[features.docs]` and `[features.test]` with two follow-up `AskUserQuestion`s; on opt-in, populate `[features.docs.standalone]` paths / `test_paths` / `lcov_paths` from a codebase survey and chain to the companion setup skill. Read-only on the codebase. | chains to `heal-doc-pair-setup` / `heal-test-reporter-setup` |
| `heal-code-review` | Code | Read every `heal status --all --json` finding, deeply investigate, return one architectural reading + prioritized refactor TODO list. Read-only — proposes only. | write counterpart `heal-code-patch` |
| `heal-code-patch` | Code | Drain the cache fixing one finding per commit in Severity order. Refuses dirty worktree. Calls `heal mark fix` after each commit. **Does not push or open PRs.** | write counterpart of `heal-code-review` |
| `heal-doc-pair-setup` | `[features.docs]` | One-shot: detect doc ⇔ src pairs (mention regex + directory mirror + optional LLM) and write `.heal/doc_pairs.json`. Read-only on source; only writes the SSoT. Manual entries are preserved across regenerations. | — |
| `heal-doc-scaffold` | `[features.docs]` | Stand up the project's wiki from nothing, autonomously, and re-runnable safely. Five-phase pipeline (Detect codebase → Survey existing tree → Reconcile → Emit → Report): re-invocation flows fresh codebase signal into auto-managed sections without disturbing hand-edits. Strict emit gate — a page lands only when the codebase can fill it with meaningful content. Tier 1 always emits; Tier 2-3 conditional pages emit when their detection trigger fires AND auto-fill is mostly real content. Skeleton-only pages are **not emitted on first run** (Quality Goals, Bounded Context Map, Roadmap, Risks, Service Overview, SLOs, Runbooks, Postmortems, On-call Onboarding, Security) — the user authors them when they have the input. `TODO(human):` markers ship inside exactly **one** file: the ADR template (`decisions/0000-template.md`). No `AskUserQuestion` calls; detection signals alone drive the emit plan. Three flags govern existing-tree behaviour: default = reconcile (per-section refresh + preserve hand-edits); `--missing-only` = additive bootstrap; `--force` = regenerate emit-set pages from scratch. Files outside the emit set are sacred in every mode. Frontmatter is one field (`title:`); state recoverable from `git log` or body content was dropped. Output under `[features.docs] scaffold_root` (default `.heal/docs/`). | precedes `heal-doc-pair-setup` for new projects |
| `heal-doc-review` | `[features.docs]` | Read every doc-family finding from `heal status --json`, frame through Diátaxis (Tutorial / How-to / Reference / Explanation), and return one architectural reading + prioritized doc-fix TODO. Read-only. | write counterpart `heal-doc-patch` |
| `heal-doc-patch` | `[features.docs]` | Drain doc findings one finding per commit (broken internal links, dangling identifier removal, orphan registration, resolvable TODOs). Allow-list / escalate-list is doc-specific. Refuses dirty worktree. | write counterpart of `heal-doc-review` |
| `heal-test-reporter-setup` | `[features.test]` | Detect language stack (Rust / Python / JS-TS / Go / Scala / mixed) and propose lcov reporter + CI config so `lcov.info` lands at one of HEAL's default `lcov_paths`. Read-only — proposes commands without running them. | — |
| `heal-test-review` | `[features.test]` | Read every test-family finding from `heal status --json`, frame through the test-pyramid lens (unit / integration / e2e), and return one architectural reading + prioritized test-fix TODO. Read-only. | write counterpart `heal-test-patch` |
| `heal-test-patch` | `[features.test]` | Drain test findings one finding per commit (writing missing unit tests for uncovered hotspots, aligning drifted tests, re-enabling skipped tests whose reason no longer holds). Runs the test suite per commit; refuses to weaken assertions or skip flakes. | write counterpart of `heal-test-review` |

Three review ↔ patch pairs (`code`, `doc`, `test`) follow the same
contract: review = read-only architectural proposal, patch = mechanical
write with one finding per commit. Don't merge them. Each `*-patch`
skill restricts its drain to the slice of `latest.json` matching its
family — `heal-code-patch` skips doc / test findings, `heal-doc-patch`
skips code / test findings, etc.

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

### Nudge format

Single line by default — post-commit output stays compact:

- No calibration → silent (no output at all).
- 0 critical / high → `heal: recorded · clean`.
- Has critical / high → `heal: recorded · X critical, Y high · heal status`.

When `[features.test.coverage]` is enabled and at least one
`coverage_pct` finding sits on a hotspot file at High / Critical
severity, a second indented line names the count
(`         · N uncovered hotspot`). The line is suppressed when the
coverage feature is off so projects that don't ingest lcov see no
extra noise.

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
