---
name: heal-doc-pair-setup
description: Scan the source tree and doc tree, detect doc ⇔ src pairs using mention-based regex, directory-mirror heuristics, and (optionally) LLM inference, then write `.heal/doc_pairs.json` as the SSoT for the `[features.docs]` observer family. Read-only on the codebase; writes only `.heal/doc_pairs.json`. Trigger on "set up doc pairs", "generate doc_pairs.json", "initialize heal docs", "/heal-doc-pair-setup".
metadata:
  heal-version: 0.3.2
  heal-source: bundled
---

# heal-doc-pair-setup

Produces (or updates) `.heal/doc_pairs.json`, the SSoT for the
`[features.docs]` observer family. Three phases: **detect** doc ↔
src correspondences via three independent heuristics, **merge**
with the existing file (preserving manual entries), **write** the
SSoT atomically. Read-only on source files; the only file written
is `.heal/doc_pairs.json`.

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

## Output language

Write progress messages, candidate-pair explanations, and the final
report in the user's language. Resolution order:

1. Explicit instruction in the current conversation.
2. The language the user is writing in (Claude Code's conversation
   language).
3. `[project].response_language` in `.heal/config.toml` (free-form:
   `"Japanese"`, `"日本語"`, `"ja"`, `"français"`).
4. English (fallback).

Identifiers stay verbatim — file paths, `source` values (`"manual"`,
`"mention"`, `"directory_mirror"`, `"llm"`), JSON field names, and
config keys (`[features.docs]`) are part of the contract. The written
file (`.heal/doc_pairs.json`) is machine-consumed JSON and never
translated; only the conversation around it follows the user's
language.

## Pre-flight

1. **`[features.docs]` enabled.** Check `.heal/config.toml`. If
   disabled (or absent), tell the user the file would have no
   consumers and ask whether to enable. If they decline, stop.
2. **Existing JSON loaded.** When `.heal/doc_pairs.json` exists,
   read it. `source: "manual"` entries are sacred and carry
   through every regeneration unchanged.
3. **Walk targets:**
   - **Sources:** files under tree-sitter-supported extensions
     (`.rs`, `.ts`, `.tsx`, `.js`, `.jsx`, `.py`, `.go`, `.scala`).
   - **Docs:** files matching `features.docs.standalone.include`
     (default `**/*.md` + `**/*.rst`) minus `exclude`.

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

##### Guardrails (avoid downstream `doc_drift` flooding)

`doc_drift` flags every backtick-span identifier in the doc that
does *not* appear in the paired srcs. The cost of a too-narrow
pair is high: a glossary mention-paired to 3 src files produces
`(distinct identifiers in doc) − (identifiers in those 3)`
false-positive findings. Apply these gates before emitting a pair:

- **Cross-cutting denylist.** Don't mention-pair docs whose value
  is *spanning* the codebase — they're handled fine by the
  standalone family (`orphan_pages`, `todo_density`,
  `doc_link_health`). Typical members: glossary / terminology
  pages, design-philosophy / prior-art / architecture-overview
  pages that name everything in passing, repo-wide READMEs
  (`README.md`, `CLAUDE.md`, `.claude/docs/README.md`,
  `.claude/rules/README.md`), workflow / scope rule pages.
- **Coverage-ratio gate.** Of the doc's *resolvable* identifiers
  (those matching some src definition in the repo), the chosen
  `srcs` set must cover **≥ 70 %**. Below that threshold the
  remaining mentions become `doc_drift` noise — drop the pair.
- **Unfocused-doc cap.** When a doc has **> 60** distinct
  resolvable identifiers, treat it as cross-cutting whether or not
  it's on the denylist. Drop the pair; let standalone handle it.
- **Per-src hit threshold.** Require ≥ 2 distinct identifier hits
  for a `(doc, src)` candidate. A single passing reference (one
  cross-link to a type defined elsewhere) is too weak to justify
  drift checks against that src.

These gates were derived from dogfooding HEAL itself: the naive
"any hit emits a pair" rule produced > 1800 critical `doc_drift`
findings on a 200-file codebase, dominated by filename /
metric-string false positives. Adding the guardrails halved the
count while preserving the genuinely mention-rich pairs
(data-model docs, observer specs, CLI references).

##### Carve-out: central-types files

Counterweight to the gates: a file that defines **central shared
types** must not be excluded from a doc's `srcs` set just because
the per-src hit threshold or the coverage-ratio gate would
otherwise drop it. Central types are the cross-cutting vocabulary
that every Reference / Explanation page in the codebase touches —
losing them inflates `doc_drift` noise on every well-written doc.

Identify central-types files in two passes over the corpus:

1. **Counted-mention sweep.** Build the
   `(identifier → set-of-docs)` index. Identifiers that appear in
   **≥ 3 distinct docs** are central vocabulary. Filter to those
   the per-`is_identifier_shape` rule would treat as identifiers
   (i.e. survive the doc-drift extractor's filters too — file
   paths, metric strings, and CLI flags don't count here).
2. **Defining-file resolution.** For each central identifier,
   intersect the set of src files that define it (via the same
   tree-sitter pass that powers the mention pair). When the
   identifier is defined in **a small set** (1–3 src files), tag
   each of those as a *central-types file*.

Then, when building each doc's `srcs`:

- Central-types files defining identifiers the doc mentions are
  **always included**, bypassing the per-src hit threshold and the
  coverage-ratio gate.
- The coverage-ratio gate's denominator excludes identifiers the
  doc mentions whose ONLY definitions live in central-types files
  (those are now automatically resolved by the carve-out — they
  shouldn't penalise the rest of the gate's accounting).

For HEAL specifically, the carve-out resolves identifiers like
`Severity`, `Finding`, `FindingsRecord`, `MetricCalibration`,
`Config`, `Family`, `IntoFindings` — defined in
`crates/cli/src/core/finding.rs`, `core/severity.rs`,
`core/calibration.rs`, `core/config.rs`. Without it, every
internal Reference doc becomes a drift hotspot for routine type
mentions even though the types are perfectly stable.

Don't hardcode a central-types list — derive it from the corpus
each run. Codebases with different shapes need different carve-out
sets.

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

For large codebases (>50k LOC, >500 docs), cap LLM inference at
the **20 unpaired docs** with shortest paths first, ties broken by
recent commit churn (`heal metrics --metric churn --json`). Surface
the truncated count in the output ("N more docs unpaired; rerun
with stronger hints to extend"); never silently truncate.

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

- **Writes `.heal/doc_pairs.json` only.** Never edits
  `config.toml` or any source / doc file.
- **Manual entries are sacred.** Re-running must never silently
  change a `source: "manual"` entry. Deletion happens only on
  explicit user request, not as auto-cleanup.
- **No auto-correct.** When the LLM pass picks the wrong pair,
  the user fixes it by editing the JSON. Don't reverse the
  edit on the next run.
- **Schema version drift.** When `DOC_PAIRS_VERSION` bumps,
  HEAL treats the old file as absent. Mention in the output if
  you detect a mismatch.
- **English output.** Summary is English; the JSON has no
  localised content.
