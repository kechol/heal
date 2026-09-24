---
name: docs
description: Work on the project's documentation from what heal's `[features.docs]` family found — broken links, docs drifting from the code, stale or missing pages, orphans, open TODOs, and (with `[features.semantic]`) pages that mix purposes, sit in the wrong section, or explain one concept twice. Reads the docs through Diátaxis, proposes fixes, rewrites, moves, and splits, and applies the ones the user approves, one commit per proposal. In scaffold mode it stands up a documentation tree from the codebase instead. Never pushes or opens a PR. Trigger on "review the docs health", "what does heal say about my docs", "fix the doc findings", "docs are out of date", "scaffold the docs", "generate the wiki", "/heal:docs".
argument-hint: "[path | finding-id | plan | scaffold [--missing-only | --force]]"
---

# /heal:docs

Two modes:

- **Scaffold** — the argument is `scaffold`, or the user asks to build a
  documentation tree from scratch: follow `references/scaffold.md` (it
  loads `page-catalog.md`, `page-templates.md`, and
  `wiki-organization.md` itself). It writes pages under
  `[features.docs] scaffold_root` and does not commit.
- **Improve** (default) — the five steps below.

Load before starting:

- `${CLAUDE_PLUGIN_ROOT}/references/cli.md` — CLI contract, output
  language.
- `${CLAUDE_PLUGIN_ROOT}/references/apply-loop.md` — proposal format,
  approval, the per-commit loop, accepts, and the report.
- `references/fixes.md` — how each finding is fixed, and what never to
  do.

Load on demand: `references/architecture.md` (Diátaxis, the four doc
traps in §4, per-metric reading rules) and `references/metrics.md` (what
each docs metric measures and how Severity is computed).

Arguments: a path narrows to findings under it; a finding id narrows to
the proposal that resolves it; `plan` stops after proposing.

## 1. Diagnose

1. `heal status --all --feature docs --json`. Exit 1 means the family is
   disabled — suggest `/heal:setup docs` and stop. When
   `.heal/doc_pairs.json` is missing, suggest the same and stop. Set
   aside accepted findings; mention `accepted_rereview` as notes.
2. Read `.heal/doc_pairs.json` (which doc claims to describe which
   sources) and `[features.docs]` in `.heal/config.toml` (freshness
   thresholds and the document globs set the lens for Severity).
3. Take the queue: T0 then T1 in ascending `drain_rank` (counted within
   the Docs family). A `doc_*` finding with `hotspot: true` sits on a
   pair whose source changes often — a stale page there misleads readers
   about the code that matters most.
4. Group by page — one page usually carries several findings — and read
   each page once, together with every paired source.
5. **Classify each page** by its Diátaxis purpose — Tutorial, How-to,
   Reference, Explanation (`architecture.md` §1). Rigor follows purpose:
   drift in a Reference or a Tutorial step is load-bearing; drift in an
   Explanation page matters less and churning it costs more.

### With `[features.semantic]`

Read every note by its confidence: at `≥ 0.9` act on it, at `0.5–0.9`
confirm it first, below `0.5` ignore it.

- `semantic.doc_kind` gives the page's dominant kind (tutorial, how_to,
  reference, explanation, changelog, adr, runbook, glossary); use it
  instead of classifying every page yourself and check the ones where it
  surprises you.
- `doc_structure.split` / `.mixed_mode` / `.merge`, `doc_placement`,
  `doc_drift.semantic`, and `doc_concept.duplicate` / `.conflict` /
  `.gap` are the editorial findings; `references/fixes.md` says how
  each becomes a proposal.
- `semantic.placement` on `orphan_pages` names the section a reader
  would look in.
- `semantic.gate` and `semantic.effort` are a second opinion on how big
  a fix is — never the decision.

## 2. Propose

An **architectural reading** of three to six lines: which purposes the
pages serve, where the decay concentrates, and whether it is one page
lagging or a structural problem (the same concept explained on three
pages, reference material mixed into tutorials, a section nobody links
to).

Then up to eight numbered **proposals** in the `apply-loop.md` format.
Beyond direct fixes, make structural proposals when the evidence agrees:

- **Split** a page that serves two purposes (`doc_structure.mixed_mode`
  or `.split`, or `doc_kind` disagreeing with the section it sits in).
- **Consolidate** a concept explained in several places into one
  canonical page and link the rest to it (`doc_concept.duplicate` /
  `.conflict`, or the same drift recurring across pages).
- **Move** pages to the section readers look in (`doc_placement`,
  `semantic.placement`).
- **Rewrite** sections the code has moved away from (`doc_freshness`,
  `doc_drift.semantic`), outlining the corrected content from the code.
- **Retire** pages no one needs (orphans with no inbound reason to
  exist). Always pair "write more" with "delete some" — doc bloat is a
  trap too.

Mark **needs a decision** where `references/fixes.md` says so. Keep
finding ids exact. Show the proposals, then any accept candidates
(`apply-loop.md` has the docs reasons) and deferred questions. With
`plan`, stop here.

## 3–5. Choose, apply, report

Follow `apply-loop.md`, with the verification and commit format from
`references/fixes.md`. Refuse the patterns under "Refusals" there even
when a proposal seems to call for them.

End the report with the next step: another `/heal:docs` round,
`/heal:setup docs` when `doc_pairs.json` needs new pairs for pages you
added or moved, or `/heal:refactor` when the drift traces back to code
that is itself hard to explain.
