---
name: heal-doc-pair-setup
description: Scan the source tree and doc tree, detect doc ⇔ src pairs using mention-based regex, directory-mirror heuristics, and (optionally) LLM inference, then write `.heal/doc_pairs.json` as the SSoT for the `[features.docs]` observer family. Read-only on the codebase; writes only `.heal/doc_pairs.json`. Trigger on "set up doc pairs", "generate doc_pairs.json", "initialize heal docs", "/heal-doc-pair-setup".
---

# heal-doc-pair-setup

One-shot skill that produces (or updates) a project's
`.heal/doc_pairs.json`. It works in three phases: **detect** doc ↔ src
correspondences via three independent heuristics, **merge** the
candidates with the existing file (preserving manual entries), and
**write** the SSoT atomically.

The HEAL binary is a deterministic *consumer* of this file; it has no
detection logic on its own. That's why generation lives here, in a
user-triggered skill, rather than inside `heal status`.

Read-only on source files. The only file it writes is
`.heal/doc_pairs.json`. The `[features.docs]` observers under `heal
status` and `heal metrics` read it back the next time they run.

## When this skill is right

- First-time setup right after enabling `[features.docs]` in
  `.heal/config.toml`: HEAL warns that `.heal/doc_pairs.json` is
  missing, and points the user here.
- The codebase's structure changed (new modules, doc tree
  reorganized) and the existing pair list misses obvious pairings.
- The user explicitly wants to add a manual pair entry and asks for
  the file's schema.

If the user just wants to know what schema the JSON uses, point them
at `references/doc-pairs-schema.md` directly — this skill is for
*deriving* a pair list, not explaining one.

## References (load on demand)

- `references/doc-pairs-schema.md` — JSON shape, version rules, and
  the meaning of every `source` value.

## Pre-flight

Before changing anything:

1. **`[features.docs]` enabled.** Check `.heal/config.toml`. If
   `[features.docs] enabled = false` (or the section is absent), tell
   the user the file would have no consumers and ask whether to
   enable the feature now. If they decline, stop — generating the
   SSoT without the feature on is busy-work.
2. **Existing JSON loaded.** If `.heal/doc_pairs.json` already
   exists, read it. The `source: "manual"` entries are sacred — they
   carry through every regeneration unchanged.
3. **Walk plan in mind.** The detection sweeps three trees:
   - **Sources:** every file under tree-sitter-supported extensions
     (`.rs`, `.ts`, `.tsx`, `.js`, `.jsx`, `.py`, `.go`, `.scala`).
   - **Docs:** every file under `features.docs.standalone.include`
     globs (default `**/*.md` + `**/*.rst`) minus the `exclude`
     globs.
   - **Layer A targets:** these are the pair sets we want to discover.

## Procedure (Detect → Merge → Write)

### Phase 1 — Detect

Run three independent passes; collect candidates with `confidence`
and `source` annotations. A pair survives if at least one pass
emits it; multiple passes raise its confidence.

#### Step 1 — Mention-based (`source: "mention"`, confidence 0.9)

For every doc file:

1. Read the body, strip fenced code blocks (` ``` `, ` ~~~ `).
2. Scan for backtick spans that resemble symbol identifiers
   (`Foo::bar`, `MyClass`, `compute_score`).
3. For each candidate identifier, grep tree-sitter source files for
   a definition matching it. The simplest signal: identifier
   appears as a leaf token in the source AST. (HEAL's `doc_drift`
   observer uses the same matcher — borrow its behaviour.)
4. Pair = (doc, src) when at least one identifier in the doc resolves
   to that src.

This pass is high-precision but low-recall: it catches references
docs and architecture docs that name specific symbols, but misses
prose-only docs.

#### Step 2 — Directory mirror (`source: "mirror"`, confidence 0.7)

Look for path symmetries: `src/foo.rs` ↔ `docs/foo.md`, `lib/foo.ts`
↔ `docs/api/foo.md`, etc. The heuristic is structural, so it works
without parsing either file. Implementation:

1. Strip the leaf extension from each src and each doc path.
2. Bucket by **basename** (final path segment without extension):
   `src/cli/cli.rs` → `cli`, `docs/cli.md` → `cli`.
3. When exactly one src and one doc share a basename, pair them.
4. When multiple files share a basename, fall back to longest
   common suffix — `crates/cli/src/cli.rs` ↔ `docs/reference/cli.md`
   match better than `crates/api/src/cli.rs` ↔ `docs/cli.md`.

This pass is medium-precision (false positives on coincidentally-
named files) and medium-recall.

#### Step 3 — LLM inference (`source: "llm"`, confidence 0.5)

Reserved for docs the first two passes missed. The user is *you*
(the agent). For each unpaired doc:

1. Read the doc's title, the first paragraph, and any major
   section headings.
2. Read the unpaired src files' module-level doc comments / file
   headers / package docstrings.
3. Decide: does this doc describe one (or several) of these src
   files?

This pass is lowest precision — surface the candidates with
confidence 0.5 and `source: "llm"` so the user can review and
either delete entries the heuristic got wrong or promote them to
`manual` when right.

#### Stopping rule

For very large codebases (>50k LOC, >500 docs), Phase 3 can balloon
into a long-running discovery loop. Cap LLM inference at the **20
docs with no Phase 1/2 match**, sorted by what's likely most
important: shortest paths first, then by recent commit churn (use
`heal metrics --metric churn --json`). Skip the rest with a comment
in the output saying *"N more docs unpaired; rerun with stronger
hints to extend"*. Don't silently truncate without surfacing the
number.

### Phase 2 — Merge

Combine the new candidates with the existing
`.heal/doc_pairs.json`:

1. **Preserve manual entries.** Any `source: "manual"` entry from
   the existing file carries through unchanged, even when Phase 1/2
   would have produced different values for the same `(doc, srcs)`
   pair.
2. **Update auto entries.** When the existing file has a non-manual
   entry that Phase 1/2/3 *re-confirms*, refresh the `confidence` /
   `source` to whichever pass yielded the highest confidence. When
   the new pass *contradicts* an old auto entry (e.g. mirror said
   `pkg/web/foo.rs ↔ docs/foo.md` last time, mention says
   `pkg/web/foo.rs ↔ docs/api/foo.md` now), prefer the new pass —
   manual edits the user wants to preserve already moved to `manual`.
3. **Drop dangling entries** (auto only). When an old auto entry's
   `doc` no longer exists on disk, drop it. When the `srcs` array
   is partly missing, prune the missing srcs but keep the entry.
   Manual entries with dangling paths get a warning in the output
   but are NOT auto-pruned — the user might be mid-rename.

### Phase 3 — Write

Write the merged result atomically:

1. **Sort entries** for stable diffs: by `doc` ascending, then by
   first `srcs` entry. Stable order keeps `git diff
   .heal/doc_pairs.json` reviewable.
2. **Write atomically.** Render the JSON with 2-space indent and a
   trailing newline. Use a temp file + rename pattern (the same one
   `Config::save` uses).
3. **Validate by re-reading.** Run
   `heal status --refresh --feature docs --json` once. The
   integrity-check warnings on stderr should match what Phase 2
   already surfaced; if anything new shows up, the merge has a
   bug — back the file out.

## Output format

End with a short summary:

```
Doc pairs:
  total:      42
  by source:  manual=5  mention=18  mirror=15  llm=4
  added:      6 new pairs since last run
  removed:    2 stale auto pairs (referenced paths gone)

Candidates flagged for review:
  - docs/migration.md → ?  (no src match, may be standalone)
  - docs/architecture.md → src/lib.rs (LLM-inferred, confidence 0.5)
```

When the LLM pass surfaces low-confidence candidates, list them
explicitly so the user can correct them by editing the JSON
directly (and changing `source` to `"manual"` to lock in the fix).

## Constraints

- **Write `.heal/doc_pairs.json` only.** Never edit `config.toml` or
  any source / doc file from this skill.
- **Manual entries are sacred.** Re-running this skill must never
  silently change a `source: "manual"` entry. The only mutation
  allowed is deleting a manual entry the user explicitly requested
  removal of (handle that as a separate flow, not auto).
- **Recommend, don't require.** If the LLM pass picks the wrong
  pair, the user fixes it by editing the JSON. Don't try to
  auto-correct on the next run — manual edits override.
- **Schema version drift.** When `DOC_PAIRS_VERSION` bumps, HEAL
  will silently treat the old file as absent (same pattern as
  `findings_cache`). Re-running this skill regenerates under the
  new version. Mention this in the output if you detect a version
  mismatch.
- **English output.** The skill writes English in its summary; the
  JSON itself has no localised content.
