---
title: Docs · Skills
description: The heal plugin's skill for [features.docs] — /heal:docs, which reviews, fixes, restructures, or scaffolds your documentation — and the /heal:setup step that maps docs to source.
---

The opt-in **Docs** family is served by two skills from the heal
Claude Code plugin: the docs step of `/heal:setup`, which builds the
doc ⇔ source map the docs observers read, and `/heal:docs`, which
works on the pages themselves. For installing the plugin, see
[Code › Skills](/heal/code/skills/).

## `/heal:setup docs` — enable the family and map docs to source

Turns `[features.docs]` on (with document globs fitted to your
repository) and writes `.heal/doc_pairs.json`, the list of which doc
describes which source files. heal only reads that file; building it
is the setup skill's job, run when you ask. `heal doctor` reports the
docs area as `todo` when the file is missing or names paths that no
longer exist, so `/heal:setup` offers the step again.

**Three heuristics** for picking pairs:

| Heuristic          | How it picks pairs                                                                                           |
| ------------------ | ------------------------------------------------------------------------------------------------------------ |
| **Mention**        | Doc body references `path/to/source.rs` or a backtick-spanned identifier that resolves to a single src file. |
| **Mirror**         | Directory layout mirrors: `docs/payments/engine.md` ↔ `src/payments/engine.ts`.                              |
| **LLM** (optional) | Claude reads the doc and candidate sources when the first two fail. It asks before doing so.                 |

Every pair records how it was found. Pairs you added by hand
(`source: "manual"`) are kept unchanged on every re-run. Only
`.heal/doc_pairs.json` (and `.heal/config.toml` when the family is
first enabled) is written.

## `/heal:docs` — review and improve the docs

Works like `/heal:refactor`, for documentation. It reads the docs
findings, proposes changes, and applies only the ones you approve —
one commit per proposal. It never pushes or opens a pull request.

1. **Diagnose.** Reads `heal status --all --feature docs --json`, the
   doc pairs, and your docs settings, then reads each flagged page
   together with the source it describes. Each page is classified by
   its **Diátaxis** purpose — tutorial, how-to, reference, or
   explanation — because that decides how much drift matters: a stale
   reference page or tutorial step misleads readers; a stale
   explanation page usually does not.
2. **Propose.** A short reading of where the docs are decaying, then
   numbered proposals, highest-priority findings first.
3. **Choose.** You pick which proposals to apply.
4. **Apply.** One commit per proposal, with the site build (when your
   project has one) and link re-checks before each commit.
5. **Report.** What changed and what is left.

What it proposes, by finding:

| Metric            | Typical proposal                                                                                     |
| ----------------- | ---------------------------------------------------------------------------------------------------- |
| `doc_link_health` | Point the link at the renamed or moved page; ask when there are several candidates.                  |
| `doc_drift`       | Replace a renamed identifier; rewrite the section when the concept itself was removed or redesigned. |
| `doc_freshness`   | Rewrite the sections the source has moved away from, outlined from the code as it is today.          |
| `doc_coverage`    | Write the missing page when the code gives the facts — or drop the pair when no page is needed.      |
| `orphan_pages`    | Link the page from the section readers would look in, move it to an archive, or retire it.           |
| `todo_density`    | Answer TODOs the code or config now answers; turn open questions into decisions for you.             |

Beyond single fixes, it proposes **structural** changes when the
evidence supports them: splitting a page that mixes purposes (a
tutorial inside a reference page), consolidating a concept explained
on several pages into one canonical page, moving pages to the section
readers look in, and retiring pages nobody needs. Choices that are
really editorial — which page keeps an explanation, whether a stale
page is rewritten or retired — are asked as questions with concrete
options.

**Refusals:** no stub pages to silence `doc_coverage` (an empty page
is worse than none), no "harmonizing" of changelogs, migration notes,
or quotations that mention old names on purpose, and no changing code
to match a drifted doc — the source is the truth; the doc is updated.

**With [Semantic (Jev)](/heal/semantic/) enabled**, the skill also
uses the page kind Jev assigned, pages that combine several documents
or mix purposes (`doc_structure`), pages filed in the wrong section
(`doc_placement`), statements the code no longer supports
(`doc_drift.semantic`), and concepts explained twice, inconsistently,
or not at all (`doc_concept`).

Arguments: a path narrows the work to findings under it, a finding id
narrows it to that finding, and `plan` stops after the proposals.

Trigger phrases: "review the docs health", "fix the doc findings",
"docs are out of date", "/heal:docs".

## `/heal:docs scaffold` — stand up a doc tree from nothing

For a project with few or no docs. It builds a documentation tree from
codebase signals alone and is safe to run any number of times. Output
lands as Markdown under `[features.docs] scaffold_root` (default
`.heal/docs/`); it does not commit.

- **Detection-driven, not interactive.** Detection signals alone
  decide which pages are written — review the result and remove pages
  you don't want.
- **Strict emit gate.** A page lands only when the codebase can fill
  it with meaningful content. Foundational pages (README, Wiki Index,
  System Context, Architecture Overview, Glossary, Getting Started)
  always emit; conditional pages emit when their trigger fires.
  Organisational pages (Quality Goals, Roadmap, Runbooks, SLOs,
  Postmortems, Security Posture) are **not written on the first run** —
  author them when you have the input.
- **Auto-fills from real signal.** Container lists from manifests,
  module responsibilities from doc comments, glossary seeds from
  exported symbols, contributing rules from CI configs, ER tables from
  migrations. Anything it can't fill confidently is left out — never
  invented owner names or made-up SLO numbers.
- **`TODO(human):` lives in one file** — the ADR template
  (`decisions/0000-template.md`).
- **Re-runnable.** By default each auto-managed section is refreshed
  from current signal while your edits are kept. `--missing-only` only
  adds new files; `--force` regenerates the pages from scratch.

The page catalog merges Diátaxis (purpose), arc42 (architecture
sections), the C4 model (zoom levels), strategic DDD (bounded
contexts), ADRs (decision records), SRE (operational pages), and
DeepWiki (patterns seen in AI-written wikis). It works even before
`[features.docs]` is enabled; its output becomes observable once the
family is turned on.

Trigger phrases: "scaffold the docs tree", "generate the wiki",
"build the documentation from scratch", "/heal:docs scaffold".
