---
description: Canonical-name enforcement for the heal codebase. Use names from .claude/docs/glossary.md verbatim; sweep renames across all consumers in the same PR.
---

# Terminology rules

The single source of canonical names is `.claude/docs/glossary.md`.
These are the rules enforcing it.

## R1. Use canonical names verbatim

In every text surface listed in `applies_to`. No synonyms, no informal
forms.

Common bites:

- `Finding`, never "issue" / "violation" / "alert" / "result".
- `FindingsRecord`, never `CheckRecord` / `Snapshot` / `Report`.
- `heal status` (renders findings) and `heal metrics` (one-shot
  recompute) — distinct, not interchangeable.
- `heal-code-review` / `heal-code-patch`, never `heal-code-check` /
  `heal-code-fix`.
- The product is **HEAL** (in titles only) and **heal** (in prose,
  commands, source). Never "Heal" or "heal-cli" as the brand.
- "workspace" is the only term for the monorepo overlay concept.
  Never "subproject" / "package" / "module".

## R2. Renames sweep in the same PR

A rename is one PR (`refactor!(...)`). The sweep covers:

- Source under `crates/cli/src/` and `crates/cli/tests/`.
- Skill bodies under `crates/cli/skills/`.
- Starlight docs (en `docs/src/content/docs/`, ja
  `docs/src/content/docs/ja/`).
- `README.md`, `CLAUDE.md`, `CHANGELOG.md`.
- `.claude/docs/glossary.md`.

Do not split into "refactor PR + docs sweep PR". The follow-up sweep
pattern is the bug, not the cure.

## R3. Don't reintroduce removed names

These names are retired. Hits in code or docs are drift to fix, not
a thing to extend:

- `state.json`, `snapshots/`, `Snapshot`, `MetricsSnapshot`.
- `checks/`, `CheckRecord`, `check_id`, `regressed_check_id`.
- `heal run`, `heal logs`, `heal snapshots`, `heal compact`,
  `heal fix`, `heal checks` subcommands.
- `skills-install.json`.
- `marketplace.json`, `.claude-plugin/`, `.claude/plugins/heal/`.
- `heal-core`, `heal-observer`, `heal-plugin-host` crates.
- `heal-code-check`, `heal-code-fix` skills.

Mentions in `CHANGELOG.md` migration notes are intentional; leave them.
Anywhere else, fix.

## R4. Two metric naming forms; never blur them

- CLI flag value: kebab-case (`--metric change-coupling`).
- JSON key and `Finding.metric`: snake_case (`change_coupling`,
  `change_coupling.symmetric`).

`MetricsConfig` field names match the JSON key form so skills can do
`payload[payload.metric]` without translation.

When adding a metric, the JSON form is canonical; the CLI form is
derived by clap's `value_enum`.

## R5. Don't invent submetric strings

`Finding.metric` strings like `change_coupling.expected` and
`change_coupling.cross_workspace` are part of the JSON contract.
Adding a new one is a schema change — bump
`FINDINGS_RECORD_VERSION` (see `invariants.md` R3).

## R6. Hotspot is a decoration, not a target

The drain target is **Critical AND `hotspot=true`** (T0 Must). The
`hotspot` Finding itself always has `Severity::Ok` — it is a
decoration carrier for findings on hotspot files.

Don't write prose ("CCN is critical → fix CCN") that treats CCN as
the target. CCN is a proxy for testability cost; the target is
"this file is hard to change AND keeps changing".

## R7. Japanese docs: plain, native, don't force-translate

For `docs/src/content/docs/ja/*` and any Japanese surface:

- Use plain, native vocabulary. Translate the **meaning**, not the
  English sentence structure word-for-word. Stiff
  translated-from-English prose ("〜することができます", excessive
  passive voice, formal connectors that don't fit the register) is
  wrong; natural Japanese is right.
- Don't force-translate domain terms. Canonical names from
  `.claude/docs/glossary.md` (`Finding`, `FindingsRecord`,
  `Severity`, `Hotspot`, `Critical`, `change_coupling`, `LCOM`, …)
  stay as the English / code form. `Severity` を「重大度」と訳すと
  混乱の元 — そのまま `Severity` で書く。コマンド名 (`heal status`)
  やファイルパス (`.heal/findings/`) も同じ。
- Concept words that have a clear, established Japanese rendering
  (e.g. "観測" for observe, "計測" for measure) can be translated.
  When in doubt, keep the English term in `code`.
- CJK spacing: do not insert ASCII spaces around Japanese characters
  (`コードベース を 観測` is wrong). Mechanical-translation tools
  produce this artifact regularly — sweep it on every PR.
