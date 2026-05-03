# Rules (`.claude/rules/`)

Prescriptive rules that Claude Code auto-loads each session
(unconditional rules) or when matching files are opened
(`paths`-scoped rules). Per
<https://code.claude.com/docs/en/memory#organize-rules-with-claude%2Frules%2F>.

| File | Loaded | Scope |
|---|---|---|
| [terminology.md](./terminology.md) | every session | global |
| [scope.md](./scope.md) | every session | global |
| [workflow.md](./workflow.md) | every session | global |
| [invariants.md](./invariants.md) | when Rust source is opened | `crates/cli/{src,tests}/**/*.rs` |
| [skills-and-hooks.md](./skills-and-hooks.md) | when skill / hook code is opened | `crates/cli/plugins/heal/skills/**`, `claude_settings.rs`, `skill_assets.rs`, hook / skill / mark-fixed command modules |

## Split with `.claude/docs/`

- `.claude/docs/` is **descriptive** — what the system *is*. Read on
  demand. Not auto-loaded.
- `.claude/rules/` is **prescriptive** — what you may not change. Auto-
  loaded into context.

When the doc and the rule disagree, the rule wins (the doc may be
stale). Open an issue if a rule itself looks wrong.

## Rule frontmatter schema

```yaml
---
description: <one-line, agent-facing>
paths:                          # optional — scope to matching files
  - "<glob>"
---
```

- `description` is the only metadata field besides `paths`.
- Without `paths`, the rule loads unconditionally each session.
- With `paths`, the rule loads only when Claude reads a matching file.
