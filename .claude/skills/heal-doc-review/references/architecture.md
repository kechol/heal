# Doc-quality architecture reference

Reasoning frame for which `doc_*` finding to act on, in what
order, and what to preserve. Applying Reference rigor to
Tutorial content (or vice versa) inflates the noise floor.

## §1 Diátaxis — four document purposes

Daniele Procida's framework (https://diataxis.fr/) splits docs into
four kinds based on what the reader is doing.

| Purpose      | Reader's mental state         | Quality bar                         |
|--------------|-------------------------------|-------------------------------------|
| Tutorial     | Learning by doing             | Every step completes, end-to-end    |
| How-to guide | Solving a known problem       | The recipe still produces the goal  |
| Reference    | Looking something up          | Exhaustive accuracy, drift = lies   |
| Explanation  | Understanding *why*           | Coherent narrative; tolerates lag   |

Mixing purposes in one doc is a frequent failure mode: a Reference
that smuggles in Explanation reads like both an incomplete reference
AND a confusing explanation. When `/heal-doc-review` spots a doc
serving two purposes, recommend a split.

### Mapping `[features.docs]` metrics to Diátaxis quality bars

- **Tutorial:** `doc_drift` is critical — a learner who follows
  obsolete code stops at the first error. `doc_freshness` matters
  proportionally; freshness lag stalls beginners.
- **How-to:** `doc_drift` matters when the dangling identifier is
  in a step the user must execute. `doc_link_health` matters too —
  a broken "see also" link breaks the recipe's escape hatch.
- **Reference:** every metric is high-priority. Reference docs are
  consulted under time pressure; drift is actively misleading.
- **Explanation:** lower bar. `doc_freshness` and `doc_drift`
  warnings on Explanation docs are fine to defer; the cost of
  churning these on every refactor exceeds the cost of mild lag.

## §2 The doc decay spiral

Cost of bad docs is non-monotone:

- **No doc:** lookup time.
- **Stale doc:** lookup time + debug time + trust erosion.
- **Untrusted doc:** docs become invisible scaffolding.

Stale > absent > absent + scaffolding. Once trust breaks, it's
expensive to restore — this is why `doc_drift` is Critical by
default.

## §3 Per-metric reading rules

### `doc_freshness`

The `src_commits_since_doc` count is "how many commits has the
source side moved since the doc last changed." Read:

- **0–5:** noise. The metric won't fire (Severity::Ok) unless the
  user tightened the floor.
- **5–20:** Medium → High. Investigate the doc's purpose:
  - Reference / Tutorial: schedule a review pass.
  - Explanation: usually fine; defer.
- **20+:** Critical. The doc almost certainly disagrees with the
  code. Even Explanation docs at this distance need a review.

The fix isn't always "rewrite the doc." Sometimes the right answer
is to delete the doc and link to the code's own docstring.

### `doc_drift` (Type 1: dangling identifier)

A doc references an identifier (`Foo::bar`, `OldStruct`) that no
longer exists in the paired src AST. Three legitimate causes:

1. **Renamed.** Most common. Fix: replace in the doc.
2. **Removed.** Rare. Fix: delete the section or replace the
   example.
3. **Moved.** The identifier moved to a different src not in the
   pair. Fix: update `.heal/doc_pairs.json` to add the new src.

Severity is Critical by default — even one dangling identifier in
a Reference page disorients readers. Per-team softer floors go
through `[policy.drain.metrics.doc_drift]`.

### `doc_coverage` (initial pass)

Pairs in `.heal/doc_pairs.json` whose `doc` doesn't exist on disk.
Severity Medium by design — pushing this to 100 % rewards empty
docstrings (the Coverage trap, §4). Read as a list of "things the
team intended to document but never did."

The fix is one of:

- Write the doc (interpretive — see `/heal-doc-pair-setup` then
  hand to the team).
- Drop the pair entry from `doc_pairs.json` if the src no longer
  needs a dedicated doc page.

### `doc_link_health`

Internal relative links (`./other.md`, `#anchor`) that don't
resolve. Severity High because:

- Broken links are a near-universal user-facing failure.
- Internal links are mechanical to fix (the target either moved
  or was deleted).

External link rot (HTTP 404, redirects) is **out of scope** here
(R5 forbids network access). Recommend `lychee` / `linkchecker` in
CI for that.

### `orphan_pages`

Layer B docs no other doc references and which aren't conventional
entry points (README.md, index.md). Severity Medium. Read:

- **Recently added orphans:** the writer probably forgot to link
  them from a TOC.
- **Long-standing orphans:** likely abandoned content; recommend
  deletion or move to an archive directory (excluded from
  `standalone.include`).

### `todo_density`

Counts `TODO / FIXME / XXX / TBD / [要確認] / [要修正]` markers per
doc. Severity Medium at ≥3 markers, High at ≥10. The marker is
author-confessed incompleteness; the fix is usually obvious because
the writer already left a hint.

## §4 Four traps to avoid in recommendations

### Coverage trap

Pushing `doc_coverage` to 100% rewards empty docstrings
(`/// TODO: document this`). Worse than no doc — reader learned
nothing AND lost trust. Never recommend "write a doc for every
public symbol" without specifying Diátaxis purpose and quality
bar.

### Autogen trap

Generated API references (`cargo doc`, `pdoc`, TypeDoc) are
necessary but not sufficient: they list facts without narrative.
Pair generation with hand-written Explanation docs ("auto-derive
the flag list; keep the rationale in `concept.md`").

### Link perfectionism

External links rot at ~5%/year. Demanding zero broken external
links incentivises removing useful citations. Recommend Web
Archive snapshots or DOIs; treat external-link findings as out
of HEAL's scope (network access is forbidden).

### Doc bloat

`orphan_pages` and markdown `duplication` exist because
adding-only growth is a failure mode. Every "write more docs"
recommendation pairs with at least one deletion / consolidation
candidate.

## §5 Prioritization heuristic

1. **Hotspot decoration overrides metric.** `doc_freshness` on a
   hotspot file outranks `doc_drift` on a sleepy one — readers
   spend their time on hot files.
2. **Within the same severity:** Reference > Tutorial > How-to >
   Explanation. Reference is consulted under time pressure.
3. **Within the same purpose:** Mechanical > Interpretive >
   Architectural. Mechanical fixes drain fast and unblock the
   harder questions.
