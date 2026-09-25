# Skills and hooks

Internal contract for the Claude Code integration. The user-facing
description lives in the Starlight pages (`code/skills.md`,
`docs/skills.md`, `test/skills.md`, `installation.md`); this is the
implementation reference. Prescriptive rules:
[`.claude/rules/skills-and-hooks.md`](../rules/skills-and-hooks.md).

---

## Big picture

HEAL's skills ship as the **`heal` Claude Code plugin** in this
repository. The CLI does not bundle, extract, or track them, so nothing
lands in a user's git tree.

```
.claude-plugin/marketplace.json     # marketplace "heal" → plugin "heal"
plugins/heal/
  .claude-plugin/plugin.json        # version == CLI version
  hooks/hooks.json                  # SessionStart → scripts/session-start.sh
  scripts/session-start.sh
  references/cli.md                 # CLI contract, output-language rules
  references/apply-loop.md          # propose → approve → commit loop
  skills/setup/                     # /heal:setup
  skills/refactor/                  # /heal:refactor
  skills/docs/                      # /heal:docs
  skills/tests/                     # /heal:tests
```

Install (user, once): `/plugin marketplace add kechol/heal`, then
`/plugin install heal@heal`. Claude Code keeps the plugin under
`~/.claude/plugins/cache/`. Teams can pre-enable it for a project in
`.claude/settings.json` (`extraKnownMarketplaces.heal.source =
{ "source": "github", "repo": "kechol/heal" }`, `enabledPlugins =
{ "heal@heal": true }`) — a user decision; the CLI never writes it.

To run the skills from a checkout of this repository instead of the
released plugin: `claude --plugin-dir plugins/heal`.

---

## Versioning

The marketplace entry uses a `git-subdir` source:

```json
{ "source": "git-subdir", "url": "https://github.com/kechol/heal.git",
  "path": "plugins/heal", "ref": "v0.6.0" }
```

`plugin.json` `version`, the marketplace entry's `version`, the source
`ref` (`v<version>`), and `workspace.package.version` in `Cargo.toml`
move together in the `/release` PR; `crates/cli/tests/plugin_manifest.rs`
pins them. Consequences:

- `/plugin marketplace add` reads `marketplace.json` from `main`, but
  the plugin content comes from the release tag — users never get
  skills that depend on unreleased CLI behaviour.
- Claude Code only offers an update when `version` changes, and
  third-party marketplaces do not auto-update by default: users run
  `/plugin marketplace update heal` + `/plugin update heal@heal`.
- Between a release PR merging and its tag being pushed, the `ref`
  names a tag that does not exist yet; installs fail for that window.

### SessionStart hook

`scripts/session-start.sh` (POSIX `sh`, always exit 0):

1. Exit silently unless `$CLAUDE_PROJECT_DIR/.heal` exists (a
   user-scope install runs it in every project).
2. Read the plugin version from `${CLAUDE_PLUGIN_ROOT}/.claude-plugin/plugin.json`.
3. `heal` missing → one message with the install commands.
4. `heal --version` in the same series (0.x: `major.minor`; 1.0+:
   `major`) → silent. Otherwise one message naming the side to upgrade.

Output is one JSON object: `systemMessage` (shown to the user) and
`hookSpecificOutput.additionalContext` (given to the model).

---

## The skills

| Skill | Replaces | Loop | Writes |
|---|---|---|---|
| `/heal:setup` | heal-setup, heal-concepts-setup, heal-doc-pair-setup, heal-test-reporter-setup | `heal doctor --json` → required (`todo`/`error`) + optional steps → run selected steps in dependency order → doctor again | `.heal/config.toml`, `.heal/doc_pairs.json`, `.heal/concepts.toml`; calibration / hook / cleanup via `heal` commands; reporter installs step-approved |
| `/heal:refactor` | heal-code-review, heal-code-patch | diagnose code findings → reading + proposals (structural when two signals agree) → approve → one commit per proposal | source code (+ the tests/docs a change breaks), `heal mark fix` / `accept` |
| `/heal:docs` | heal-doc-review, heal-doc-patch, heal-doc-scaffold | same loop through Diátaxis; `scaffold` mode builds a doc tree | docs, `heal mark` |
| `/heal:tests` | heal-test-review, heal-test-patch | same loop through the test pyramid | tests, `heal mark` |

Each skill loads `${CLAUDE_PLUGIN_ROOT}/references/cli.md` first. The
three work skills share `references/apply-loop.md`:

- **Proposal format** — Change / Why / Resolves (finding ids) / Touches
  / Risk; "needs a decision" for public renames, contested boundaries,
  editorial or harness choices.
- **Choose** — `AskUserQuestion` multi-select; nothing is written
  before approval.
- **Pre-flight** — clean worktree; verification commands pass on the
  clean tree.
- **Apply** — re-read, change, verify (tests + family extras), commit,
  `verify_patch` (semantic), `heal mark fix` per targeted finding,
  `heal status --refresh --feature <family>`; stop on relocation.
- **Accept** — `heal mark accept` with a categorical reason, per-finding
  approval; the reason table lives in `apply-loop.md`.

Family-specific material stays in each skill's `references/`:
`refactor/references/{metrics,architecture,readability}.md`,
`docs/references/{fixes,architecture,metrics,scaffold,page-catalog,page-templates,wiki-organization}.md`,
`tests/references/fixes.md`, and for setup
`setup/references/{tuning,config,docs,doc-pairs-schema,tests,semantic}.md`.

### Structural proposals (`/heal:refactor`)

A structural change is an ordinary proposal when two independent
signals agree on the seam, for example `concept_mix` groups matching
LCOM clusters, `concept_misplaced` plus change coupling with the target
file, or a hub in many coupling pairs that also mixes concepts. One
signal alone makes it a deferred question. This is the gap the old
review/patch split left: review only proposed, patch only applied a
mechanical allow-list, so Extract Class / Move Function / file splits
were never carried out.

---

## CLI side

### `heal doctor` (`commands/doctor.rs`)

Read-only, offline setup report; `/heal:setup` is its main consumer.
Contract and drift rules: [commands.md](./commands.md#heal-doctor).

### `heal init`

Writes `.heal/`, `config.toml` (kept unless `--force`), the post-commit
hook, and `calibration.toml` (kept unless `--force`). No skill install.
The summary prints the plugin commands and, when
`legacy_skills::find` returns anything, a pointer to
`heal skills uninstall`; `--json` lists them under `legacy_skills`.
`--yes` / `--no-skills` are hidden no-ops.

### `heal skills uninstall` (`commands/skills.rs`)

1. Remove every directory `legacy_skills::find` returns — the closed
   `NAMES` list under `ROOTS` (`.claude/skills`, `.agents/skills`).
   User skills with other names stay.
2. Remove now-empty skill roots and `.agents/`.
3. `claude_settings::unregister`: sweep `LEGACY_HEAL_COMMANDS` hook
   entries from `.claude/settings.json`, the `heal-local` marketplace
   keys, `.claude/plugins/heal/`, and `.claude-plugin/marketplace.json`
   **only when it parses with `"name": "heal-local"`**.

JSON: `{ removed: [project-relative paths], claude_settings:
"updated" | "unchanged" }`. `install` / `update` / `status` are hidden
and exit 1 with the plugin pointer.

---

## Post-commit git hook

Installed by `heal init` to `.git/hooks/post-commit` with marker
`# heal post-commit hook`:

```sh
#!/usr/bin/env sh
# heal post-commit hook
if command -v heal >/dev/null 2>&1; then
  heal hook commit || true
fi
exit 0
```

- Failures swallowed (`|| true`) → never blocks a commit.
- Skips entirely if `heal` is not on `PATH`.
- `heal hook commit` silently no-ops if `.heal/` doesn't exist.

The hook stays in git (not in the plugin) because it must also fire on
commits made outside Claude Code, and it can refresh coverage via
`[features.test.coverage].post_commit_refresh`. `.git/hooks/` is not
tracked, so it adds nothing to the user's tree.

### Nudge format

Single line by default:

- No calibration → silent.
- 0 critical / high → `heal: recorded · clean`.
- Has critical / high → `heal: recorded · X critical, Y high · heal status`.

With `[features.test.coverage]` enabled, a second indented line reports
uncovered hotspots (`         · N uncovered hotspot`) or partial
coverage observation.

---

## Test helpers (`test_support.rs`)

- `git_bin()` — cached `OnceLock` PATH lookup with fallbacks.
- `git(cwd, args)` — shell out, panic on nonzero.
- `init_repo(cwd)` — `git init -q` + `commit.gpgsign=false`.
- `commit(cwd, file, body, email, msg)` — write + add + commit.
- `init_project_with_config(dir, source)` — repo + one commit +
  `.heal/` with a default config.

---

## What you must **not** do

- Don't reintroduce skill bundling, extraction, `metadata:` injection,
  drift detection, or a sidecar manifest in the CLI.
- Don't let the CLI write `enabledPlugins` / `extraKnownMarketplaces`
  or any hook entry into `.claude/settings.json`.
- Don't add hooks beyond post-commit (git) and SessionStart (plugin),
  and don't let the SessionStart hook do more than compare versions.
- Don't split a work skill into read-only and write-only halves, or let
  it write before the user approved a proposal.
- Don't let the plugin version drift from the CLI version.
- Don't widen `legacy_skills::NAMES` or `LEGACY_HEAL_COMMANDS`
  speculatively.
