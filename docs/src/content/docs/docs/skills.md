---
title: Docs · Skills
description: The three bundled Claude Code skills for [features.docs] — /heal-doc-pair-setup, /heal-doc-review, /heal-doc-patch.
---

The opt-in **Docs** family ships three Claude skills, extracted
alongside the Code-family skills on `heal skills install` /
`heal init`. They only act on findings produced by the docs
observers.

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
