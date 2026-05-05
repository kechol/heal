---
title: Docs · Skills
description: The four bundled skills for [features.docs] — /heal-doc-pair-setup, /heal-doc-scaffold, /heal-doc-review, /heal-doc-patch. Available for Claude Code and OpenAI Codex.
---

The opt-in **Docs** family ships four skills, extracted alongside
the Code-family skills for every detected agent target on
`heal init`. They only act on findings produced by the docs
observers, with one
exception: `/heal-doc-scaffold` works even on projects that don't
enable `[features.docs]` — its output just becomes observable
once the family is turned on.

For installation and the drift-aware update model, see
[Code › Skills](/heal/code/skills/) — the mechanism is shared.

## `/heal-doc-pair-setup` — write the pair file

One-shot setup skill. Scans the source tree and doc tree, detects
doc ⇔ src pairs, merges with the existing pair file (preserving
manual entries), and writes `.heal/doc_pairs.json` atomically.
heal is a deterministic **consumer** of this file; it has no
detection logic of its own, which is why generation lives in
this user-triggered skill rather than inside `heal status`.

**When to run it:** first-time setup right after enabling
`[features.docs]` (heal warns when the file is missing); after a
significant tree restructure that breaks existing pairings; or
when adding a manual pair entry by hand.

**Three heuristics** for picking pairs:

| Heuristic | How it picks pairs |
|---|---|
| **Mention** | Doc body references `path/to/source.rs` or a backtick-spanned identifier that resolves to a single src file. |
| **Mirror** | Directory layout mirrors: `docs/payments/engine.md` ↔ `src/payments/engine.ts`. |
| **LLM** (optional) | An LLM read of doc + candidate source, when the first two fail. Skipped by default; the skill asks before invoking. |

Each candidate carries a `confidence` score and a `source` field.
The merge pass preserves every `source: "manual"` entry unchanged.
Read-only on source files; only `.heal/doc_pairs.json` is written.

Trigger phrases: "set up doc pairs", "generate doc_pairs.json",
"initialize heal docs", "/heal-doc-pair-setup".

## `/heal-doc-scaffold` — stand up the wiki from nothing

Bootstrap skill, safe to invoke any number of times. Five phases
— Detect codebase → Survey existing tree → Reconcile → Emit →
Report — pull current codebase signal into the documentation
tree without disturbing hand-edits. Output lands as Markdown
under `[features.docs] scaffold_root` (default `.heal/docs/`).

The skill's contract:

- **Detection-driven, not interactive.** Detection signals alone
  decide what emits — no per-page menus. Review the emitted tree
  and remove pages you don't want; one review action, not many
  prompts.
- **Strict emit gate.** A page lands only when the codebase can
  fill it with meaningful content. Foundational pages (README,
  Wiki Index, System Context, Architecture Overview, Glossary,
  Getting Started) always emit; conditional pages emit when their
  trigger fires AND auto-fill is mostly real content.
  Organisational / forward-looking pages (Quality Goals, Roadmap,
  Runbooks, SLOs, Postmortems, Security Posture) are **not
  emitted on first run** — author them when you have the input.
- **Auto-fills from real signal.** Container lists from manifests,
  module responsibilities from doc comments, glossary seeds from
  exported symbols, contributing rules from CI configs, ER tables
  from migrations. Cells the skill can't fill confidently are
  **not emitted at all** — invented owner names or made-up SLO
  numbers are forbidden.
- **`TODO(human):` lives in one file** — the ADR template
  (`decisions/0000-template.md`).
- **Idempotent.** Default mode reconciles per-section:
  auto-managed sections refresh from current signal, hand-edits
  are preserved, user-added sections pass through. `--missing-only`
  only adds new files; `--force` regenerates emit-set pages from
  scratch (overrides hand-edits).
- **Minimal frontmatter** — one field per page (`title:`).

The page catalog merges Diátaxis (purpose), arc42 (architecture
sections), C4 model (zoom levels), strategic DDD (Bounded
Contexts), ADR (decision records), SRE (operational pages), and
DeepWiki (empirical AI-Wiki regularities).

Trigger phrases: "scaffold the docs tree", "generate the wiki
skeleton", "build the documentation from scratch",
"/heal-doc-scaffold".

## `/heal-doc-review` — the audit skill

Read-only. Reads `heal status --json`, filters to the
`[features.docs]` slice, and returns:

1. An **architectural reading** of the doc tree — is the dominant
   axis "tutorial drifted from the actual install steps", "API
   reference stale", or "concept docs link-broken"?
2. A **prioritized doc-fix TODO list** — Tutorial / How-to drift
   first (a confused first-time reader is the highest-leverage
   fix), then Reference, then Explanation.

Per-metric framing through the **Diátaxis** lens:

| Metric | Diátaxis question |
|---|---|
| `doc_freshness` | Has the section the user reads first moved? |
| `doc_drift` | Will a copy-pasted snippet still compile? |
| `doc_coverage` | Is this source expected to have docs at all? |
| `doc_link_health` | Will internal navigation work? |
| `orphan_pages` | Is this page reachable from real entry points? |
| `todo_density` | Is this doc under construction or abandoned? |

Never edits source. After reading a review you can act on any
item right away — ask the agent in the same session ("fix the
broken links", "rewrite the install section"). Mechanical
breakage flows through `/heal-doc-patch`; rewrites that need a
human voice ("is this section still relevant?", "should this
how-to be split?") wait for your direction.

### Why review and patch are split

**Patch** handles the mechanical: a link that no longer
resolves, a renamed identifier, a page nobody links to. **Review**
additionally surfaces the items that need a human read — whether
a stale tutorial should be rewritten, deleted, or merged; whether
a reference drift is a real bug or a deliberately simplified
example. Mixing the two would either rush past judgment calls or
refuse to fix the easy stuff.

Trigger phrases: "review the docs health", "where should we fix
documentation", "/heal-doc-review".

## `/heal-doc-patch` — the write skill

Drains the docs slice of `.heal/findings/latest.json` one finding
at a time. **One commit per fix.**

**Pre-flight** (refuses to start otherwise):

- Clean worktree.
- Cache exists (runs `heal status --json` to populate if missing).
- `[features.docs]` enabled in `.heal/config.toml`.
- `.heal/doc_pairs.json` exists when `doc_freshness` /
  `doc_drift` / `doc_coverage` findings are in scope (otherwise
  points the user at `/heal-doc-pair-setup`).

**Per-metric moves:**

| Metric | Default move |
|---|---|
| `doc_link_health` (`MissingPath`) | Update the relative path. On a rename, follow `git log --diff-filter=R`. |
| `doc_link_health` (`MissingAnchor`) | Match the heading slug; update the link if the heading was renamed. |
| `doc_drift` | Remove the stale reference, or restore the identifier under its new name on a clear rename. |
| `orphan_pages` | Add a link from the parent README; if the orphan should be deleted, escalate. |
| `todo_density` | Resolve the resolvable TODOs; escalate the rest to GitHub issues with a link in the doc. |
| `doc_freshness` | Re-read the paired source and rewrite the affected section. Preserve voice and structure — this is a content sync, not a redesign. |
| `doc_coverage` | Escalate to the user — the patch skill won't write a brand-new doc unilaterally. |

**Refusals** (encoded in the skill body):

- **Stub-without-content** — never writes a one-line file just to
  silence `doc_coverage`.
- **Cosmetic-pass** — the per-commit fix targets the specific
  Finding; no drive-by sentence rewrites or "while I'm here"
  reformatting.
- **Doc-as-truth-source** — never updates code to match a drifted
  doc. Source is canonical; the doc gets updated.

**Constraints**: one finding = one commit, Conventional Commit
subject + `Refs: F#<finding_id>` trailer, never push / amend /
`--no-verify`. Findings whose metric belongs to the Code or Test
families are skipped.

Trigger phrases: "fix the doc findings", "drain the doc cache",
"patch stale docs", "/heal-doc-patch".
