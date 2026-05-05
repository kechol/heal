---
title: Docs · Metrics
description: The six documentation-quality metrics the [features.docs] family produces, plus Markdown duplication.
---

The opt-in **Docs** family adds six metrics on top of the always-on
Code family. Each one targets a specific way documentation drifts
from the source it describes.

For configuration knobs see [Docs › Configuration](/heal/docs/configuration/).
For the bundled skills see [Docs › Skills](/heal/docs/skills/).

## At a glance

| Metric | Layer | What it flags | Severity |
|---|---|---|---|
| `doc_freshness` | A (paired) | source commits since the paired doc last changed | configurable floors (default ≥ 5 High, ≥ 20 Critical) |
| `doc_drift` | A (paired) | doc references an identifier no longer in the paired source | uniform Critical |
| `doc_coverage` | A (paired) | pair entry whose `doc` path doesn't exist on disk | uniform Medium |
| `doc_link_health` | A + B | internal relative-path / `#anchor` link doesn't resolve | uniform High |
| `orphan_pages` | B (standalone) | Layer B doc not linked from anywhere; not paired | uniform Medium |
| `todo_density` | A + B | `TODO` / `FIXME` / `XXX` / `TBD` / `[要確認]` / `[要修正]` markers per doc | ≥ 3 Medium, ≥ 10 High |

**Layer A** (paired docs) and **Layer B** (standalone prose docs)
are different observer scopes. Layer A needs a doc ⇔ src mapping
in `.heal/doc_pairs.json`; the bundled `/heal-doc-pair-setup`
skill generates it. Layer B is auto-discovered via the
`standalone` include / exclude globs.

## `doc_freshness`

> _"Has the source moved since the doc was last touched?"_

Per-pair source-commits-since-paired-doc count. Distance is
measured in **git commits**, not wall-clock days, so the threshold
doesn't shift as the team's commit pace changes.

Severity is decided by the absolute floors set in
`[features.docs.doc_freshness]`:

| `src_commits_since_doc ≥` | Severity (default) |
|---|---|
| 20 | Critical |
| 5  | High |
| 1  | Medium |
| 0  | Ok (no Finding emitted) |

See [Docs › Configuration](/heal/docs/configuration/#features.docsdoc_freshness)
for tuning.

## `doc_drift` (Type 1: dangling identifier)

> _"Does the doc still reference identifiers that exist in the
> source?"_

Scans each Layer A doc, extracts identifier-shaped backtick spans
(`` `Foo::bar` ``, `` `processOrder` ``), and emits a Finding for
each one that doesn't resolve to any identifier in the paired
source.

**Severity:** Critical. A reader acting on a missing identifier
wastes time looking for code that no longer exists; the fix is
mechanical (remove the reference, or restore the identifier under
a new name).

**Out of scope for v0.4** (deferred to v0.5+):

- Type 2 — signature mismatch. The function still exists but the
  parameters don't match the doc's example.
- Type 3 — semantic drift. The function exists with the same
  signature but the doc's description is wrong about what it does.

## `doc_coverage`

> _"Does the paired doc actually exist on disk?"_

Pair entries from `.heal/doc_pairs.json` whose `doc` path doesn't
exist. Severity is **uniform Medium** by design — if it were
Critical, it would incentivise empty-stub manufacturing (write a
one-line file just to satisfy the metric).

Medium says "consider writing this", not "you must". The fix is
either real content or `heal mark accept` recording the "won't
doc" decision.

## `doc_link_health`

> _"Do the internal links in the docs resolve?"_

Scans Layer A docs and the standalone Layer B walk; emits a
Finding per:

- `MissingPath` — a relative-path link that doesn't resolve to a
  file on disk.
- `MissingAnchor` — a same-doc `#anchor` (or another-doc
  `path.md#anchor`) that doesn't match any heading in the target.

Heading slugs follow the GitHub-style slugify convention
(lowercase + non-alnum → `-`).

**Severity:** High — internal breaks are mechanical to fix and
high-impact for readers.

**Out of scope:** External HTTP links. heal is local-only;
`lychee` and similar tools cover HTTP in CI.

## `orphan_pages`

> _"Which Layer B docs aren't reached from anywhere?"_

Layer B docs (per `[features.docs.standalone]`) that aren't linked
from any other Layer B doc and aren't paired (Layer A pairs are
implicitly reachable via the pair file).

Conventional entry points are seeded as "linked" so they don't
trip the metric:

- `README.md` at any depth.
- `index.md` at any depth.

Both are reachable from outside the doc graph (a directory
listing, a docs-site index page) and shouldn't surface as orphans
just for being top-level.

**Severity:** Medium. An orphan doc isn't broken, just hard to
discover. The fix is usually a one-line link from the parent
README, not a rewrite.

## `todo_density`

> _"How many open TODOs is each doc carrying?"_

Per-doc count of `TODO` / `FIXME` / `XXX` / `TBD` / `[要確認]` /
`[要修正]` markers. Markers inside fenced code blocks are
excluded — those are illustrative, not real action items. Markers
inside backtick-quoted inline-code spans are excluded by default
too (`[features.docs.todo_density] ignore_in_inline_code = true`),
so a reference page that *describes* the marker keywords
(e.g. this very document) doesn't self-flag every paragraph.

| `marker_count ≥` | Severity |
|---|---|
| 10 | High |
| 3  | Medium |
| ≤ 2 | Ok (no Finding) |

The count → Severity floors are hard-coded in v0.4. The inline-code
skip toggle and a per-doc `allowlist_paths` glob list can be tuned
in `[features.docs.todo_density]` (see
[Configuration](/heal/docs/configuration/#featuresdocstodo_density)).

## Markdown duplication

When `[features.docs]` is on, the Duplication observer adds a
parallel pass over Markdown / RST files. Findings land under the
same `duplication` metric string as the code-side blocks; the
distinguisher is the file extension.

Tokenisation differs from the code path: word-split + lowercased,
fenced code blocks stripped, so prose tokens can't collide with
code tokens.

The use case: spotting docs that have been copy-pasted across
language mirrors (en + ja), across module-specific READMEs, or
across API reference pages with shared boilerplate. The fix is
usually a "see also" link plus a single canonical source.

Window length is `[metrics.duplication].docs_min_tokens` (default
100). See [Docs › Configuration](/heal/docs/configuration/#markdown--rst-duplication-window).

## `doc_hotspot` — which paired doc is most worth updating

`doc_hotspot` is the docs-family analogue of code Hotspot. It
ranks **paired** doc ↔ src entries by
`paired_src_churn × debt`, where:

```
debt = src_commits_since_doc + weight_drift × dangling_idents
```

A pair scores high when the paired source is changing fast
**and** the doc has fallen behind (commits-since-doc, dangling
identifiers, or both). High score = "of all your docs, this is
the next one worth updating".

Only paired entries from `doc_pairs.json` are scored. Standalone
prose docs (READMEs, concept guides) are out of scope here —
`orphan_pages` and `todo_density` cover them with their own
signals.

`doc_hotspot` itself always carries `Severity::Ok`; it decorates
the docs-family Findings (`doc_freshness`, `doc_drift`,
`doc_coverage`, `doc_link_health`, `todo_density`) on the doc
side and on every paired src so a `doc_drift` Finding flagged on
`docs/api.md` and a `doc_freshness` Finding flagged on the same
pair both pick up `hotspot=true` together.

Default graduation gate is `[features.docs.hotspot] floor_ok = 5`
(roughly "2 commits × 2 debt units"). `weight_drift` defaults to
`1.0` — raise it (e.g. to `5.0`) if factually-wrong docs
(dangling identifiers) should outrank merely-stale ones in the
same project.

## How `/heal-doc-review` and `/heal-doc-patch` use these

`/heal-doc-review` reads `heal status --json`, filters to the docs
family, and frames the findings through the **Diátaxis** lens
(Tutorial / How-to / Reference / Explanation):

- Tutorial / How-to drift first (a confused first-time user is
  the highest-leverage fix).
- Reference drift next (the audience is high-frequency).
- Explanation drift last (less time-critical).

`/heal-doc-patch` drains the docs slice of the cache one finding
per commit:

- **`doc_link_health`** → fix the link (relative path or anchor
  slug).
- **`doc_drift`** → remove the stale reference, or restore the
  identifier under its new name when there's a clear rename.
- **`doc_freshness`** → re-read the paired source and update the
  doc to match.
- **`orphan_pages`** → add a link from the parent README, or
  delete the orphan.
- **`todo_density`** → resolve the resolvable TODOs, escalate the
  rest to issues.
- **`doc_coverage`** → write a stub doc, or `heal mark accept` if
  the team has decided this source doesn't need its own doc.

See [Docs › Skills](/heal/docs/skills/) for the full contracts.
