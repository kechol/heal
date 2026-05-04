---
title: Docs · Configuration
description: How to enable [features.docs], pick which standalone docs to scan, tune the freshness floors, and understand the .heal/doc_pairs.json file.
---

The **Docs** family is opt-in. Off by default — turn it on when
you want heal to surface stale documentation alongside the code
metrics. External HTTP link checking and example-code execution
stay out of scope (heal is local-only — `lychee` / `linkchecker`
cover the HTTP side in CI).

For what each metric flags, see [Docs › Metrics](/heal/docs/metrics/).
For the bundled skills, see [Docs › Skills](/heal/docs/skills/).

## Quick enable

```toml
[features.docs]
enabled = true
```

Then run `/heal-doc-pair-setup` once to populate
`.heal/doc_pairs.json`. heal is a read-only consumer of that file
— see [`.heal/doc_pairs.json`](#heal-doc_pairsjson--the-pair-file)
below.

## `[features.docs]`

```toml
[features.docs]
enabled    = false                           # master switch
pairs_path = ".heal/doc_pairs.json"          # SSoT location
```

- `enabled` (default `false`) — master switch. While false, every
  docs observer is a no-op and `.heal/doc_pairs.json` is not
  consulted.
- `pairs_path` (default `.heal/doc_pairs.json`) — project-relative
  path to the pair file. heal only reads it; generation is the
  `/heal-doc-pair-setup` skill's job.

## `[features.docs.standalone]`

```toml
[features.docs.standalone]
include = ["**/*.md", "**/*.rst"]
exclude = [
  "CHANGELOG*", "CHANGELOG/**",
  "CONTRIBUTING*",
  "CODE_OF_CONDUCT*",
  "SECURITY*",
  "**/adr/**",
  "target/**", "dist/**", "node_modules/**",
]
```

`standalone` covers **Layer B** docs — prose pages (READMEs,
concept guides, explanation pages) that need link / orphan / TODO
checks but don't need pair matching.

The default `exclude` list cuts:

- Governance / history files (`CHANGELOG*`, `CONTRIBUTING*`,
  `CODE_OF_CONDUCT*`, `SECURITY*`) — drift detection doesn't apply
  to dated history.
- ADRs (`**/adr/**`) — each entry is dated and not edited after
  merge by convention.
- Generated API reference and build artifacts.

Add to `exclude` when you have generated docs the defaults don't
catch (e.g. a `docs/api-generated/` tree).

## `[features.docs.doc_freshness]`

```toml
[features.docs.doc_freshness]
high_commits     = 5    # source commits past doc → High severity
critical_commits = 20   # source commits past doc → Critical severity
```

Absolute commit-distance floors. Distance is measured in commits,
not days, so the threshold doesn't shift as the team's commit pace
changes.

Apply rule:

| `src_commits_since_doc ≥` | Severity |
|---|---|
| `critical_commits` | Critical |
| `high_commits` | High |
| 1 | Medium |

Tighten by lowering both floors; loosen by raising them.

## `.heal/doc_pairs.json` — the pair file

The pair file is **tracked in git** alongside `config.toml` and
`calibration.toml` so teammates on the same commit see the same
pairing universe. heal never auto-generates it.

```json
{
  "version": 1,
  "pairs": [
    {
      "doc": "docs/architecture.md",
      "srcs": ["src/lib.rs", "src/observer/mod.rs"],
      "confidence": 0.92,
      "source": "mention"
    },
    {
      "doc": "docs/payments.md",
      "srcs": ["src/payments/engine.ts"],
      "confidence": 1.0,
      "source": "manual"
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `version` | Schema version (currently `1`). |
| `pairs[].doc` | Project-relative path to a documentation file. |
| `pairs[].srcs` | One or more source files the doc describes. |
| `pairs[].confidence` | `0.0` – `1.0`. Manual entries are usually `1.0`; auto-detected entries carry the heuristic's confidence. |
| `pairs[].source` | One of `"mention"` (doc references the src), `"mirror"` (directory layout mirrors), `"llm"` (LLM inference), `"manual"` (user-authored — preserved across regeneration). |

**Manual entries are sacred.** When `/heal-doc-pair-setup`
regenerates the file, every `source: "manual"` row is preserved
unchanged. Only the auto-detected rows are recomputed.

Integrity is best-effort:

- Doc path missing on disk → surfaces as a `doc_coverage` Finding.
- Src path missing on disk → surfaces as a `doc_drift` Finding (the
  doc references identifiers that no longer exist).

## Markdown / RST duplication window

When `[features.docs]` is on, the Duplication observer adds a
parallel pass over Markdown / RST files. The window length is
tuned in `[metrics.duplication]`, not under `[features.docs]`,
because the underlying observer is `Duplication`:

```toml
[metrics.duplication]
docs_min_tokens = 100        # Markdown / RST window
```

- `docs_min_tokens` (default `100`) — minimum window length for
  the Markdown / RST pass. Tokenisation differs from the code
  path: word-split + lowercased, fenced code blocks stripped.

## Strict by design

Like every other section, `[features.docs]` and its children
reject unknown keys:

```toml
[features.docs.standalone]
includes = ["**/*.md"]   # ✘ unknown — heal errors here
                          #   (it's `include`, singular)
```
