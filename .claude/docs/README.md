# Internal docs (`.claude/docs/`)

AI agents working in this repo: read these before touching code. They cover
internal contracts that are **not** in `docs/` (Starlight site, user-facing)
or `README.md`.

| Doc | Read when… |
|---|---|
| [glossary.md](./glossary.md) | You are about to introduce a new term, rename one, or unsure which spelling is canonical. **Single source of truth for vocabulary.** |
| [design-philosophy.md](./design-philosophy.md) | You're about to add a feature that changes a load-bearing tenet (single record, no LLM in CLI, no cool-down, hotspot ⊥ severity). |
| [architecture.md](./architecture.md) | You need the layered picture: where in the call graph your change lands. |
| [data-model.md](./data-model.md) | You touch `.heal/findings/`, `Config`, `Calibration`, `Finding`, schema versions. |
| [observers.md](./observers.md) | You add or modify a metric observer, or change classification. |
| [commands.md](./commands.md) | You add or modify a CLI subcommand, output shape, or exit code. |
| [skills-and-hooks.md](./skills-and-hooks.md) | You touch `crates/cli/plugins/heal/skills/`, `claude_settings.rs`, `skill_assets.rs`, or the post-commit hook. |
| [conventions.md](./conventions.md) | Error handling, atomic writes, FNV-1a, `deny_unknown_fields`, tests, lints. |
| [prior-art.md](./prior-art.md) | You are about to add an observer or refactoring pattern. Lineage, what was rejected, what is out-of-scope. |

For **rules** ("things you must / must not do, derived from past sessions"),
see `.claude/rules/`. The split is deliberate:

- `.claude/docs/` — *descriptive*: what the system **is**, today.
- `.claude/rules/` — *prescriptive*: what you **may not change** without
  breaking a contract or repeating a mistake the team already made.

User-facing docs (`docs/` Starlight site, `README.md`, `CLAUDE.md`) are the
authoritative external contract. Where this internal documentation conflicts
with them, the external docs win — fix this tree.
