# Doc-quality architecture reference

Use this when reasoning about which `doc_*` finding to act on, in
what order, and what to preserve while doing so. The vocabulary is
load-bearing: applying Reference rigor to Tutorial content (or vice
versa) inflates the noise floor and trains users to ignore HEAL's
docs findings.

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

The cost of *bad* docs is non-monotone:

- **No doc:** reader doesn't know X exists. Cost = lookup time.
- **Stale doc:** reader trusts the doc, then discovers it's wrong.
  Cost = lookup time + debug time + trust erosion.
- **Untrusted doc:** reader sees the warning, ignores docs going
  forward. Cost = the docs become invisible scaffolding.

Stale > absent > absent + scaffolding. Once doc trust breaks, the
cost of restoring it is very high. This is why `doc_drift` is
Critical by default — an actively-misleading doc is worse than no
doc.

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

Pushing `doc_coverage` toward 100% rewards empty docstrings:

```rust
/// TODO: document this
pub fn frobnicate() {}
```

This is *worse* than no doc — the reader learned nothing AND lost
trust in the doc system. **Don't recommend "write a doc for every
public symbol" without specifying the doc's Diátaxis purpose and
quality bar.**

### Autogen trap

Generated API references (`cargo doc`, `pdoc`, TypeDoc) are
necessary but not sufficient. They list every type and signature
but never explain *why*. A doc strategy that's only generated
output reads as a comprehensive list of facts with no narrative —
readers can find anything and understand nothing.

When recommending fixes, pair generation with hand-written
Explanation docs: "auto-derive the flag list; keep the rationale
in `concept.md`."

### Link perfectionism

External links rot at ~5 % per year. A doc strategy that demands
zero broken external links incentivises rewriting around the
problem — removing useful citations to avoid the linkchecker
warning. The result: docs that don't reference the source
material readers would find most useful.

For external sources, recommend Web Archive snapshots (`web.archive.org`)
or DOIs where they exist. Tag external-link findings as out of
HEAL's scope; this skill should not propose mass-removal of
citations.

### Doc bloat

The deletion-side metrics (`orphan_pages`, `duplication` over
markdown) exist because adding-only growth is a failure mode.
Every "write more docs" recommendation must come paired with at
least one deletion / consolidation candidate. If the prioritized
TODO has only writes, the docs strategy is unsustainable.

## §5 Prioritization heuristic

When ordering the prioritized TODO:

1. **Hotspot decoration overrides metric.** A `doc_freshness` finding
   on a hotspot file outranks a `doc_drift` finding on a sleepy
   one. Readers spend their time on hot files; that's where bad
   docs do the most damage.
2. **Reference > Tutorial > How-to > Explanation** within the same
   metric severity. Reference is consulted under time pressure;
   correctness is load-bearing.
3. **Mechanical > Interpretive > Architectural** within the same
   doc purpose. The mechanical fixes drain quickly under
   `/heal-doc-patch` and unblock the harder questions.
