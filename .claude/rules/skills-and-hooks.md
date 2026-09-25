---
description: Constraints on the Claude Code integration layer — the heal plugin (skills, SessionStart hook, marketplace), the post-commit git hook, legacy cleanup, hidden mark group.
paths:
  - "plugins/heal/**"
  - ".claude-plugin/**"
  - "crates/cli/src/legacy_skills.rs"
  - "crates/cli/src/claude_settings.rs"
  - "crates/cli/src/commands/init.rs"
  - "crates/cli/src/commands/skills.rs"
  - "crates/cli/src/commands/doctor.rs"
  - "crates/cli/src/commands/hook.rs"
  - "crates/cli/src/commands/mark.rs"
---

# Skills and hooks rules

## R1. Two hooks, one each side

- **git post-commit**, installed by `heal init` into `.git/hooks/`.
  Fires on every commit, human or agent.
- **Claude Code SessionStart**, shipped by the plugin
  (`plugins/heal/hooks/hooks.json` → `scripts/session-start.sh`). It
  only compares `heal --version` with the plugin version.

Nothing else. No `Stop`, `PostToolUse`, `Edit`, `Notification`, or
`PreCompact`, and the CLI never writes hook entries into
`.claude/settings.json`. If your idea adds a hook, redirect it to (a)
post-commit nudge content, (b) `heal doctor` output, or (c) a skill
step the user invokes.

## R2. Post-commit hook never blocks a commit

```sh
heal hook commit || true
```

The `|| true`, the `command -v heal` guard, and the marker
`# heal post-commit hook` are load-bearing:

- A broken HEAL install must never break the user's commit flow.
- The marker is what makes the install idempotent without clobbering
  user-authored hooks.

## R3. Hooks are silent on uninitialised projects

`heal hook commit` returns 0 without printing when `.heal/` or
`config.toml` is missing. `session-start.sh` exits 0 without output
when `$CLAUDE_PROJECT_DIR/.heal` does not exist — a user-scope plugin
install runs it in every project. Both always exit 0. Startup hints
belong in `heal doctor` / `heal status`, not here.

## R4. The SessionStart hook compares release series only

0.x: `major.minor` must match; 1.0+: `major`. On mismatch it prints one
JSON object (`systemMessage` + `hookSpecificOutput.additionalContext`)
naming which side to upgrade. It never runs `heal` subcommands other
than `--version`, never touches `.heal/`, and never uses the network.
POSIX `sh` only — no `jq`, no bash-isms.

## R5. Plugin version == CLI version, marketplace pinned to the tag

`plugins/heal/.claude-plugin/plugin.json` `version`, the marketplace
entry's `version`, and `workspace.package.version` in `Cargo.toml` are
the same string; the marketplace entry's `git-subdir` source `ref` is
`v<version>`. `/release` bumps all four together and
`crates/cli/tests/plugin_manifest.rs` fails CI when they drift. Users
therefore only ever receive the skills of a tagged release; `main` may
run ahead of the published plugin.

## R6. Skill layout and naming

```
plugins/heal/
  .claude-plugin/plugin.json
  hooks/hooks.json
  scripts/session-start.sh
  references/{cli,apply-loop}.md      # shared; skills load via ${CLAUDE_PLUGIN_ROOT}
  skills/{setup,refactor,docs,tests}/SKILL.md (+ references/)
```

Claude Code namespaces plugin skills as `<plugin>:<dir>`, so the
directory name is the invocation (`skills/refactor/` → `/heal:refactor`).
SKILL.md `name` equals the directory name. Adding, renaming, or
removing a skill is a user-visible contract change (`!` commit) and
sweeps docs per `terminology.md` R2.

## R7. One work skill per family; review and patch stay merged

`/heal:refactor` (code), `/heal:docs`, and `/heal:tests` each run the
same loop defined in `plugins/heal/references/apply-loop.md`:
diagnose → propose → the user approves → one commit per approved
proposal → report. Don't split a family back into read-only and
write-only skills, and don't let a skill write before the user approved
the proposal.

Loop rules — encoded in `apply-loop.md`, don't relax:

- Proposals follow `drain_rank` (T0 first; never re-derive, never mix
  families).
- Structural proposals need two independent signals on the same seam;
  public renames, contested boundaries, and DDD strategic moves are
  marked "needs a decision" and asked individually.
- Refuses to write on a dirty worktree.
- One approved proposal per commit; `heal mark fix` once per resolved
  finding, all with that commit's SHA.
- With `[features.semantic]`, `verify_patch` (and `verify_tests` for
  tests) runs after each commit; a failing verdict undoes it.
- Never pushes, opens a PR, amends, or skips hooks.

## R8. `/heal:setup` is idempotent and doctor-driven

Every run starts from `heal doctor --json`; required steps come from
`todo` / `error` sections, everything else is optional. It never
recalibrates without the user's yes, never stores an API key, never
runs the paid `heal semantic ask` unless asked, and writes only
`.heal/config.toml`, `.heal/doc_pairs.json`, and `.heal/concepts.toml`
directly (calibration, hook, and cleanup go through `heal` commands).
Drift thresholds live in `commands/doctor.rs`, not in the skill.

## R9. Skills are Claude Code skills

Skill bodies may name Claude Code tools (`AskUserQuestion`) and use
`${CLAUDE_PLUGIN_ROOT}` / `${CLAUDE_SKILL_DIR}`. Other agents are not a
target (Codex users may copy the directories by hand, unsupported).
Descriptions stay trigger-rich and end with the slash form
(`/heal:setup`). Validate with
`claude plugin validate --strict plugins/heal/skills` and
`claude plugin validate --strict .`.

## R10. The CLI does not bundle or install skills

No `include_dir!`, no extraction, no `metadata:` injection, no drift
detection, no sidecar manifest. `heal skills` keeps only `uninstall`,
which removes the closed list in `legacy_skills::NAMES` under
`legacy_skills::ROOTS` and runs `claude_settings::unregister`.
`install` / `update` / `status` stay parseable (hidden) and exit 1
with the plugin pointer. `heal init --yes` / `--no-skills` stay
accepted as hidden no-ops.

## R11. Legacy sweeps are closed and content-checked

- `legacy_skills::NAMES` lists only names a released HEAL wrote. Add
  to it only when retiring a name that actually shipped.
- `LEGACY_HEAL_COMMANDS` (`heal hook edit`, `heal hook stop`) is closed
  the same way.
- `.claude-plugin/marketplace.json` is removed only when it parses as
  the `heal-local` marketplace HEAL once wrote. A project that is itself
  a marketplace (this repository is one) must keep its manifest.

## R12. The `heal mark` group is hidden from `--help`

`#[command(hide = true)]` on the group. `mark fix` and `mark accept`
are agent-facing — surfacing them invites running them without the
workflow that gives the entry meaning (a commit for `fix`, an approved
`reason` for `accept`).

`heal mark-fixed` is the deprecated v0.2 alias: hidden, delegates to
`heal mark fix` after a stderr warning that points at the plugin and
`heal skills uninstall`. Don't remove it without a major-version bump.
