# Doc fix patterns

Loaded by `/heal:docs` while building proposals and applying them.
Three groups: **direct fixes** (low risk, one reading of the doc and its
paired source is enough), **rewrites and moves** (real editorial work —
proposals the user approves, often marked **needs a decision**), and
**refusals** (never do these, whatever the finding says).

## Direct fixes (risk: low)

### `doc_link_health` — missing path

The link `[text](./old-path.md)` doesn't resolve. Fix it when the target
was renamed and the new path is unambiguous (one candidate in
`git log --diff-filter=R`), or deleted with a clear redirect in the same
directory. Several candidates → a rewrite proposal instead.

### `doc_link_health` — missing anchor

A heading with the right text exists under a different slug → fix the
anchor. The heading was renamed and the new slug is obvious → update
target and anchor.

### `doc_drift` — renamed identifier

The identifier is a leftover from a rename and the codebase has one
clear successor: replace it in fenced examples and in prose, keeping the
sentence true. When the identifier was removed entirely, delete the
sentence or example if the surrounding text still reads correctly.

### `orphan_pages`

Link the page from the obvious slot in the index or navigation
(`docs/SUMMARY.md`, `_sidebar.md`, the Starlight / mdBook / mkdocs nav).
With `[features.semantic]`, `semantic.placement` names the section a
reader would look in. Archived content moves to an excluded directory
(`docs/archive/**`). Never link a page from an arbitrary doc just to
clear the finding — a link is a claim that the pages belong together.

### `todo_density` — answerable markers

`TODO: pin Rust version` when the codebase now pins it; `[要確認]
サポートする最小バージョン` when the answer is in `Cargo.toml`. Replace
the marker with the answer the code or config gives today.

### `doc_placement` (confidence ≥ 0.9)

Move the page (`git mv`) to the section the note names, fix every
inbound link, and update the site navigation — all in one commit.

## Rewrites and moves (risk: medium or high)

These change what a page says or how the tree is organized. Propose them
with a short outline of the new text or structure, grounded in what the
paired source does now.

- **`doc_freshness`** — the doc lags its source. Read the source's
  commits since the doc last changed and propose the corrected sections,
  or the page's retirement when nothing in the code needs it anymore
  (**needs a decision**).
- **`doc_coverage`** — a pair's doc is missing. Propose writing it only
  when the code gives the facts; state the page's Diátaxis purpose and
  audience in the proposal (**needs a decision** when the purpose is
  unclear). Or propose dropping the pair via `/heal:setup docs` when
  the source no longer needs its own page.
- **`doc_drift` without a successor** — the concept was removed or
  redesigned; rewrite the section instead of substituting a name.
- **`doc_drift.semantic`** — a section states something the code no
  longer does; propose the corrected statement, quoting the code that
  shows it.
- **Ambiguous redirects** and **unanswerable TODOs** (`TODO: decide
  whether to deprecate this`) — **needs a decision**, or a tracked
  issue.
- **`doc_structure.split`** — several documents share one page; propose
  one page per document along the line ranges in the summary. A split
  whose note calls a boundary "close to the threshold" is **needs a
  decision**. **`doc_structure.mixed_mode`** — move the minority mode
  (for example a tutorial walkthrough inside a reference page) to its
  own page and link it. **`doc_structure.merge`** — join short
  neighbouring pages that continue one document.
- **`doc_concept.duplicate` / `.conflict`** — one concept explained in
  several places, or explained differently: keep one canonical
  explanation and link to it from the others (for a conflict, check
  the code for which explanation is true). **`doc_concept.gap`** — a
  concept the code relies on that no doc explains: propose where it
  belongs and what it must say.
- **Findings on hotspot pairs** — the paired source is complex and
  changing; check whether the drift reflects an unfinished redesign
  before patching the doc.

## Refusals

- **Manufactured content.** No stub pages or empty sections to satisfy
  `doc_coverage` (`# CLI\n\nCLI documentation.` is worse than no page —
  the coverage trap, `architecture.md` §4).
- **Harmonizing intentional drift.** Quotations, migration guides,
  deprecation notes, and CHANGELOG entries mention old names on
  purpose; leave them.
- **Machine translation.** Rewrite in the language the page is written
  in; don't translate pages into another language unasked.

## Verification

Most doc trees have no build step, so check by hand as well:

- After a link fix, search all docs for the old target; the fix must
  not break another link.
- Build the site when the project has a generator (Starlight, mdBook,
  mkdocs, Sphinx) and confirm it passes.
- After answering a TODO or rewriting a section, confirm each statement
  against the code or config as it is today.

## Commit message

`docs(<scope>): <verb> <what> in <doc-path>` with a body naming the
cause (the rename, the removal, the redesign) and one
`Refs: F#<finding-id>` trailer per targeted finding. Commit messages
stay in the project's commit language even when the doc is not.
