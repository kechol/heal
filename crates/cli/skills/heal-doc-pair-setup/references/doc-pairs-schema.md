# `.heal/doc_pairs.json` schema reference

SSoT for Layer A doc ⇔ src pair mappings. Read by `heal status` /
`heal metrics` when `[features.docs] enabled = true`; written only
by `/heal-doc-pair-setup`.

## File location

Default: `.heal/doc_pairs.json` (project-root relative).
Configurable via `[features.docs] pairs_path = "..."` in
`.heal/config.toml`. Override only when the default conflicts
with an existing tool.

## Top-level shape

```json
{
  "version": 1,
  "pairs": [
    {
      "doc": "docs/cli.md",
      "srcs": ["crates/cli/src/cli.rs"],
      "confidence": 0.9,
      "source": "mention"
    }
  ]
}
```

## Fields

### `version` (required, integer)

Schema version. Bump on any breaking change to the JSON shape.
Older files (`version` mismatch with the binary's `DOC_PAIRS_VERSION`)
are silently treated as absent — `heal status` prints a warning and
the user re-runs `/heal-doc-pair-setup`.

`DOC_PAIRS_VERSION` is currently `1`.

### `pairs[]` (required, array)

One element per doc ⇔ src(s) mapping. Order is for human reading
only — the observers don't depend on it. The skill writes pairs
sorted by `doc` then by first `srcs` entry, so `git diff
.heal/doc_pairs.json` produces minimal diffs across re-runs.

### `pairs[].doc` (required, string)

Project-relative path to the doc file, forward-slash form (`docs/cli.md`,
not `docs\cli.md`). The doc is what readers land on; finding output
points at it.

### `pairs[].srcs` (required, array of strings)

One or more project-relative source files this doc describes
(observers treat the array as a set; order is informational).
A single doc can cover several srcs — e.g. a CLI reference page
covers `cli.rs` plus files under `commands/`.

The set is **what a reader of the doc would expect to read
next** — *not* a transitive closure of every type the doc
mentions. `Foo::bar` referenced in passing for context doesn't
mean `Foo`'s defining file joins the pair.

### `pairs[].confidence` (optional, float in `[0.0, 1.0]`)

Detection confidence. Conventional ranges:

- `0.9` — mention-based pass produced a high-confidence symbol
  match.
- `0.7` — directory-mirror heuristic matched.
- `0.5` — LLM-inferred, user should review.
- omitted / `1.0` — manual.

Optional because manual entries don't need a confidence field.

### `pairs[].source` (optional, string enum)

How the entry was produced. Values:

- `"mention"` — a doc backtick-spans an identifier defined in the
  src.
- `"mirror"` — directory layout mirrors src to doc.
- `"llm"` — model-inferred (the `/heal-doc-pair-setup` skill's
  Phase 3 pass).
- `"manual"` — user-authored or user-promoted. **Preserved across
  regeneration.**

## Manual-entry contract

`source: "manual"` is a one-way door — Phase 2 (Merge) treats
manual entries as immutable. To change one, edit the JSON
directly. To lock in a good LLM inference, edit it and flip
`source` to `"manual"`; the skill preserves it from then on.

## Integrity warnings

`DocPairsFile::integrity_check` runs on every `heal status` with
docs enabled. Each missing path produces one non-fatal stderr
warning:

```
warn: .heal/doc_pairs.json: pair[3] references missing path src/cli_old.rs
```

Observers skip the offending entry and continue. Manual entries
with missing paths are **not** auto-removed — the user re-adds
the moved file deliberately.

## Common shapes

### One doc covers a single src

```json
{
  "doc": "docs/cli.md",
  "srcs": ["crates/cli/src/cli.rs"],
  "source": "mention",
  "confidence": 0.9
}
```

### One doc covers a directory

```json
{
  "doc": "docs/observers.md",
  "srcs": [
    "crates/cli/src/observer/churn.rs",
    "crates/cli/src/observer/duplication.rs",
    "crates/cli/src/observer/hotspot.rs"
  ],
  "source": "manual"
}
```

### Manual entry overriding a misdetection

```json
{
  "doc": "docs/architecture.md",
  "srcs": ["crates/cli/src/observers.rs"],
  "source": "manual",
  "confidence": 1.0
}
```

(`/heal-doc-pair-setup`'s mention pass might pair this with
`crates/cli/src/feature.rs` because the doc references `Feature` —
but the user knows the doc actually describes the orchestrator
layer in `observers.rs`, so they edited the entry and locked it
with `manual`.)
