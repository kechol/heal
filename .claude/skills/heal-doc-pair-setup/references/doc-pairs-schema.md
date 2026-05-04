# `.heal/doc_pairs.json` schema reference

The single source of truth for Layer A doc ⇔ src pair mappings. The
HEAL binary reads this file at every `heal status` / `heal metrics`
invocation when `[features.docs] enabled = true`. It does not write
it — generation lives in `/heal-doc-pair-setup`.

## File location

Default: `.heal/doc_pairs.json` (project root relative). The path is
configurable via `[features.docs] pairs_path = "..."` in
`.heal/config.toml`. The default is the right answer for almost
every project; change only when the convention conflicts with an
existing tool.

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

One or more project-relative source files this doc describes. A
single doc can describe several srcs — a CLI reference page often
covers a `cli.rs` plus several files under `commands/`. Order is
informational; the observers treat the array as a set.

The set should be **what a reader of the doc would expect to read
next**. It is *not* a transitive closure of every type the doc
mentions; `Foo::bar` mentioned in passing for context doesn't mean
`Foo`'s defining file should join the pair.

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

`source: "manual"` is a one-way door: once an entry carries it, the
skill's Phase 2 (Merge) treats the entry as immutable. To change a
manual entry, edit the JSON directly. To convert a poor LLM
inference into a stable pair, edit the entry and change `source`
to `"manual"` — the skill will preserve it from then on.

## Integrity warnings

At every `heal status` invocation with the docs feature enabled,
the binary calls `DocPairsFile::integrity_check`. Each referenced
path that doesn't exist on disk produces one stderr warning:

```
warn: .heal/doc_pairs.json: pair[3] references missing path src/cli_old.rs
```

The warnings are non-fatal. The observers skip the offending entry
and continue. Manual entries with missing paths are **not** auto-
removed — they survive the warning so the user can re-add the moved
file deliberately.

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
