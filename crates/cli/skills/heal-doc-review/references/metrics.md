# `[features.docs]` metrics reference

Per-metric definitions matching the binary's observer outputs. Use
this when interpreting `Finding.summary` / `Finding.metric` strings
out of `heal status --json`.

## `doc_freshness`

| Field | Value |
|---|---|
| Layer | A (paired) |
| Trigger | `src_commits_since_doc > 0` |
| Default severity | floor cascade: ≥20 → Critical, ≥5 → High, ≥1 → Medium |
| Calibration | absolute floors in `[features.docs.doc_freshness]` |
| Hotspot decoration | possible — file-level `Finding.location` |

`Finding.location.file` is the **doc** path (the side that needs
updating). `Finding.locations[]` carry the paired src files for
context. The summary reads `"src has moved N commit(s) since doc
last changed"`.

Fix shape: read the src side at HEAD, compare against what the doc
claims, edit the doc to reflect current behaviour. Don't claim a
change happened — the metric measures distance, not correctness.

## `doc_drift`

| Field | Value |
|---|---|
| Layer | A (paired) |
| Trigger | doc backtick span references identifier absent from paired src AST |
| Default severity | Critical (uniform) |
| Calibration | not percentile-driven; binary signal |
| Hotspot decoration | possible |

`Finding.location.file` is the doc, `Finding.location.line` is the
line carrying the dangling backtick span. `Finding.summary` includes
the offending identifier. `Finding.locations[]` carry the paired
src files where the AST was scanned.

Fix shape:

- Identifier was renamed: replace in the doc.
- Identifier was removed: delete the doc section (or convert to
  past-tense migration note).
- Identifier moved to a different src: update
  `.heal/doc_pairs.json` to include the new src in the pair.

## `doc_coverage`

| Field | Value |
|---|---|
| Layer | A (paired) |
| Trigger | pair entry's `doc` path does not exist on disk |
| Default severity | Medium (uniform) |
| Calibration | not percentile-driven |
| Hotspot decoration | possible (on the src side) |

`Finding.location.file` is the **src** that should have a doc.
`Finding.locations[]` carry the expected `doc` path. The summary
includes the missing path.

Fix shape: write the doc (and recommend the user pick a Diátaxis
purpose first), or remove the pair entry from
`.heal/doc_pairs.json` if the src no longer needs a dedicated
page.

## `doc_link_health`

| Field | Value |
|---|---|
| Layer | A + B |
| Trigger | relative path or `#anchor` doesn't resolve |
| Default severity | High (uniform) |
| Calibration | not percentile-driven |

`Finding.location.file` and `Finding.location.line` point at the
doc + line carrying the broken link. `Finding.summary` distinguishes
two break kinds:

- `MissingPath` — the relative path doesn't resolve to a file.
- `MissingAnchor` — the `#anchor` doesn't match any heading slug
  in the same doc.

External links (`http://`, `https://`, `mailto:`) are not checked
— that's a CI / `lychee` job, not HEAL's.

Fix shape: edit the link target. Often the target was renamed and
the new path can be inferred from `git log --diff-filter=R`.

## `orphan_pages`

| Field | Value |
|---|---|
| Layer | B |
| Trigger | doc not linked from any other Layer B doc and not paired |
| Default severity | Medium |

Conventional entry points (`README.md`, `index.md` at any depth)
are never flagged — their reachability comes from outside the doc
graph (GitHub repo home, Starlight / mdBook home, etc.).

Fix shape: link from a TOC, move to an archive directory excluded
from `standalone.include`, or delete.

## `todo_density`

| Field | Value |
|---|---|
| Layer | A + B |
| Trigger | per-doc count of `TODO / FIXME / XXX / TBD / [要確認] / [要修正]` |
| Default severity | ≥10 → High, ≥3 → Medium, else Ok |

Markers inside fenced code blocks (` ``` `, ` ~~~ `) are not
counted — those are illustrative and shouldn't drive the doc-
quality signal.

Fix shape: read each marker, decide whether the answer is now
known (write it), still unknown (defer to a tracked issue), or
the marker is stale (delete).
