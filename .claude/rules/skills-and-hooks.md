---
description: Constraints on the Claude Code integration layer — bundled skills, settings.json sweep, post-commit git hook, hidden mark-fixed.
paths:
  - "crates/cli/skills/**"
  - "crates/cli/src/skill_assets.rs"
  - "crates/cli/src/claude_settings.rs"
  - "crates/cli/src/commands/init.rs"
  - "crates/cli/src/commands/skills.rs"
  - "crates/cli/src/commands/hook.rs"
  - "crates/cli/src/commands/mark_fixed.rs"
---

# Skills and hooks rules

## R1. HEAL registers exactly one hook: post-commit (git)

No `SessionStart`, `Stop`, `PostToolUse`, `Edit`, `Notification`, or
`PreCompact` in `.claude/settings.json`. The `claude_settings::wire`
machinery exists only to **sweep** legacy entries from older HEAL
versions.

If your idea adds a new hook, redirect it to (a) post-commit nudge
content or (b) a skill-driven flow the user opts into via
`/heal-...`.

## R2. Post-commit hook never blocks a commit

```sh
heal hook commit || true
```

The `|| true`, the `command -v heal` guard, and the marker
`# heal post-commit hook` are all load-bearing. Don't change them
without understanding the contract:

- A broken HEAL install must never break the user's commit flow.
- The marker is what makes the install idempotent without clobbering
  user-authored hooks.

## R3. The hook is silent on uninitialised projects

If `.heal/` doesn't exist, `heal hook commit` returns 0 immediately
without printing. Same for missing `config.toml`. Don't add startup
hints here — put them in `heal status` instead.

## R4. Skill drift is a function of bytes, not state

```
canonical(on-disk SKILL.md) != bundled raw bytes  →  drift
```

`canonical()` strips the `metadata:` block from SKILL.md frontmatter;
other files compared verbatim. There is no `skills-install.json`,
no SHA store, no timestamp file.

The retired sidecar caused drift verdicts to diverge across teammates
re-installing on different machines. Don't reintroduce it.

## R5. `metadata:` block in SKILL.md is auto-managed

In source SKILL.md, the frontmatter has `name` and `description` and
**not** a `metadata:` block. `skill_assets::extract` injects:

```yaml
metadata:
  heal-version: <env!("CARGO_PKG_VERSION") at compile time>
  heal-source: bundled
```

on extract. Hand-editing `metadata:` in source is overwritten on
every build.

## R6. `LEGACY_HEAL_COMMANDS` is a closed list

```rust
const LEGACY_HEAL_COMMANDS: &[&str] = &["heal hook edit", "heal hook stop"];
```

Add to it only when removing a hook entry shape we actually shipped.
Don't add speculatively — sweeping more aggressively could remove
user hooks that happen to share a name.

## R7. heal-code-review and heal-code-patch are distinct

Don't merge them. (See `scope.md` R8 for the role boundary.)

`heal-code-patch` rules — encoded in the skill body, don't relax:

- One finding per commit, in Severity order.
- Refuses dirty worktree.
- Calls `heal mark-fixed` after each commit.
- Does not push, does not open a PR.
- No `--metric` filter (the point is to drain the cache).

## R8. `heal mark-fixed` is hidden from `--help`

`#[command(hide = true)]`. Called only by `/heal-code-patch`. Don't
expose — the safe-fix flow is the skill, which calls `mark-fixed`
correctly. Surfacing it as a top-level command invites users to run
it without committing the fix, breaking the audit trail.

## R9. Skill source location

Source: `crates/cli/skills/<skill>/`. The path is **inside the crate
dir** so `cargo publish` includes it. Don't move it out —
`include_dir!` is a compile-time read of files under
`$CARGO_MANIFEST_DIR`.

## R10. Trigger-rich descriptions

Skill `description` fields are long, list trigger phrases, and end
with the slash-command form (`/heal-config`). This pattern is what
Claude Code's skill matcher keys on.

When adding a new skill, follow the existing `description` shape;
don't shorten it for terseness.
