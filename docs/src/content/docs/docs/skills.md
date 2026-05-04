---
title: Docs · Skills
description: The four bundled Claude Code skills for [features.docs] — /heal-doc-pair-setup, /heal-doc-scaffold, /heal-doc-review, /heal-doc-patch.
---

The opt-in **Docs** family ships four Claude skills, extracted
alongside the Code-family skills on `heal skills install` /
`heal init`. They only act on findings produced by the docs
observers, with one exception: `/heal-doc-scaffold` works even
on projects that don't enable `[features.docs]` — its output
just becomes observable once the family is turned on.

For installation and the drift-aware update model, see
[Code › Skills](/heal/code/skills/) — the mechanism is shared.

## `/heal-doc-pair-setup` — write the pair file

One-shot setup skill. Scans the source tree and doc tree, detects
doc ⇔ src pairs, merges with the existing pair file (preserving
manual entries), and writes `.heal/doc_pairs.json` atomically.

heal is a deterministic **consumer** of this file; it has no
detection logic on its own. That's why generation lives in this
user-triggered skill rather than inside `heal status`.

### When this skill is right

- First-time setup right after enabling `[features.docs]`. heal
  warns that `.heal/doc_pairs.json` is missing and points the
  user here.
- The codebase's structure changed (new modules, doc tree
  reorganised) and the existing pair list misses obvious pairings.
- The user wants to add a manual pair entry and asks for the
  file's schema.

### Three heuristics

| Heuristic | How it picks pairs |
|---|---|
| **Mention** | Doc body references `path/to/source.rs` or a backtick-spanned identifier that resolves to a single src file. |
| **Mirror** | Directory layout mirrors: `docs/payments/engine.md` ↔ `src/payments/engine.ts`. |
| **LLM** (optional) | An LLM read of doc + candidate source, when the first two fail. Skipped by default; the skill asks before invoking. |

Each candidate carries a `confidence: 0.0–1.0` and a
`source` field. The merge pass preserves every `source: "manual"`
entry unchanged.

### What it writes

Only `.heal/doc_pairs.json`. Read-only on source files. The
`[features.docs]` observers under `heal status` and `heal metrics`
read it back the next time they run.

Trigger phrases: "set up doc pairs", "generate doc_pairs.json",
"initialize heal docs", "/heal-doc-pair-setup".

## `/heal-doc-scaffold` — stand up the wiki from nothing

Bootstrap skill, safe to invoke any number of times. Five
phases — Detect codebase → Survey existing tree → Reconcile →
Emit → Report — flow current codebase signal into the
documentation tree without disturbing hand-edits. Output lands
as Markdown under `[features.docs] scaffold_root`
(default `.heal/docs/`).

The skill's contract:

- **Detection-driven, not interactive.** Detection signals alone
  decide what emits — no `AskUserQuestion`, no per-page menus.
  The user reviews the emitted tree and removes pages they
  don't want; one review action, not many prompts.
- **Strict emit gate — no skeleton-only pages.** A page lands
  only when the codebase can fill it with meaningful content.
  Tier 1 (README, Wiki Index, System Context, Architecture
  Overview, Glossary, Getting Started) always emits. Tier 2-3
  conditional pages (Module Map, Feature Catalog, ADR
  Index + Template, Contributing, Runtime Views, API
  Reference, Data Model, Deployment View, Crosscutting
  Concepts, Test Strategy) emit when their trigger fires AND
  auto-fill is mostly real content. Pages whose value is
  organisational / forward-looking / incident-reactive
  (Quality Goals, Bounded Context Map, Roadmap, Risks
  Register, Service Overview, SLOs, Runbooks, Postmortems,
  On-call Onboarding, Security Posture) are **not emitted on
  first run** — the user authors them when they have the
  input.
- **Auto-fills aggressively.** Container lists from manifests,
  module responsibilities from doc comments, glossary seeds
  from exported symbols, contributing rules from CI configs,
  ER tables from migrations, runtime diagrams from detected
  entry points. Cells the skill can't fill confidently are
  **not emitted at all** — invented owner names, made-up SLO
  numbers, hand-waved security policy are forbidden.
- **`TODO(human):` lives in one file.** The ADR template
  (`decisions/0000-template.md`) is the only emitted file
  carrying `TODO(human):` markers — they cue the writer when
  they copy the template to file the next ADR.
- **Idempotent — safe to re-run.** Default mode reconciles
  per-section: auto-managed sections refresh from current
  codebase signal, hand-authored sections are preserved
  verbatim, user-added sections are passed through. Flags:
  `--missing-only` (additive bootstrap; only new files);
  `--force` (regenerate emit-set pages from scratch — overrides
  hand-edits, explicit user choice). Files outside the emit
  set are sacred in every mode.
- **Minimal frontmatter.** One field per page — `title:`. No
  `diataxis` / `audience` / `freshness_owner` /
  `last_review` / `review_cycle` / `related_*` arrays:
  recoverable from `git log` or already in the body.
- **Caps at ≈ 28 page types.** Six-category top-level layout
  (Quick Start / Architecture / Reference / Operations /
  Decisions / Contributing) keeps navigation flat.

The page catalog merges Diátaxis (purpose), arc42 (architecture
sections), C4 model (zoom levels), strategic DDD (Bounded
Contexts), ADR (decision records), SRE (operational pages),
and DeepWiki (empirical AI-Wiki regularities). For the lineage,
see the skill's `references/page-catalog.md` and
`references/wiki-organization.md`.

Trigger phrases: "scaffold the docs tree", "generate the wiki
skeleton", "build the documentation from scratch",
"/heal-doc-scaffold".

## `/heal-doc-review` — the audit skill

Read-only. Reads `heal status --json`, filters to the
`[features.docs]` slice, and returns:

1. An **architectural reading** of the doc tree — what the
   findings say _as a system_. Is the dominant axis "tutorial
   drifted from the actual install steps", "API reference stale",
   or "concept docs link-broken"?
2. A **prioritized doc-fix TODO list** — Tutorial / How-to drift
   first (a confused first-time user is the highest-leverage fix),
   then Reference drift, then Explanation drift.

Per-metric framing through the **Diátaxis** lens:

| Metric | Diátaxis question |
|---|---|
| `doc_freshness` | Has the section the user reads first moved? |
| `doc_drift` | Will a copy-pasted snippet from this doc still compile? |
| `doc_coverage` | Is the audience for this source expected to find docs at all? |
| `doc_link_health` | Will internal navigation work? |
| `orphan_pages` | Is this page reachable from the entry points the audience uses? |
| `todo_density` | Is this doc actively under construction or quietly abandoned? |

`/heal-doc-review` proposes only — it never edits source. The
write counterpart is `/heal-doc-patch`.

Trigger phrases: "review the docs health", "where should we fix
documentation", "/heal-doc-review".

## `/heal-doc-patch` — the write skill

Drains the docs slice of `.heal/findings/latest.json` one finding
at a time. **One commit per fix.**

Pre-flight (refuses to start when these fail):

1. Clean worktree.
2. Cache exists (runs `heal status --json` to populate if missing).
3. `[features.docs]` enabled in `.heal/config.toml`.
4. `.heal/doc_pairs.json` exists when `doc_freshness` /
   `doc_drift` / `doc_coverage` findings are in scope. The user
   is pointed at `/heal-doc-pair-setup` if not.

Per-metric drain pattern:

| Metric | Default move |
|---|---|
| `doc_link_health` (`MissingPath`) | Update the relative path. If the target was renamed, follow `git log --diff-filter=R` to find the rename. |
| `doc_link_health` (`MissingAnchor`) | Match the heading slug; if the heading was renamed, update the link. |
| `doc_drift` | Remove the stale reference, or restore the identifier under its new name when `git log -S` finds a clear rename. |
| `orphan_pages` | Add a link from the parent README. If the orphan should be deleted, escalate to the user. |
| `todo_density` | Resolve the resolvable TODOs (e.g. "TODO: link to API ref" once the ref exists); escalate the rest to GitHub issues with a link in the doc. |
| `doc_freshness` | Re-read the paired source and rewrite the affected doc section. The rewrite preserves voice and structure — this is a content sync, not a redesign. |
| `doc_coverage` | Escalate to the user. The patch skill won't write a brand-new doc unilaterally (avoids the empty-stub trap). |

Refusals encoded in the skill body:

- **Stub-without-content** — never writes a one-line file just to
  silence `doc_coverage`. Either real content or escalate.
- **Cosmetic-pass** — the per-commit fix targets the specific
  Finding. No drive-by sentence rewrites or "while I'm here"
  reformatting.
- **Doc-as-truth-source** — never updates code to match a drifted
  doc. The source is canonical; the doc gets updated.

Constraints (enforced by the skill):

- One finding = one commit.
- Conventional Commit subject + body + `Refs: F#<finding_id>`
  trailer.
- Never push, never amend, never `--no-verify`.

`/heal-doc-patch` skips findings whose metric belongs to the Code
or Test families.

Trigger phrases: "fix the doc findings", "drain the doc cache",
"patch stale docs", "/heal-doc-patch".
