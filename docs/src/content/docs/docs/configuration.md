---
title: Docs · Configuration
description: How to enable [features.docs], pick which standalone docs to scan, tune the freshness floors, configure the scaffold root, and understand the .heal/doc_pairs.json file.
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

Then run the bundled pair-setup skill once to populate
`.heal/doc_pairs.json`. heal is a read-only consumer of that
file — see [`.heal/doc_pairs.json`](#healdoc_pairsjson--the-pair-file)
below.

```sh
claude /heal-doc-pair-setup
```

## `[features.docs]`

```toml
[features.docs]
enabled       = false                        # master switch
pairs_path    = ".heal/doc_pairs.json"       # SSoT location
scaffold_root = ".heal/docs"                 # /heal-doc-scaffold output root
```

- `enabled` (default `false`) — master switch. While false, every
  docs observer is a no-op and `.heal/doc_pairs.json` is not
  consulted.
- `pairs_path` (default `.heal/doc_pairs.json`) — project-relative
  path to the pair file. heal only reads it; generation is the
  `/heal-doc-pair-setup` skill's job.
- `scaffold_root` (default `.heal/docs`) — project-relative root
  the `/heal-doc-scaffold` skill writes Markdown skeletons into.
  heal itself never reads or writes this tree — the field is
  consumer metadata so teammates regenerating the scaffold land
  in the same place. The default keeps output under the `.heal/`
  umbrella so it doesn't collide with any existing `docs/`
  directory the project owns. Once you've reviewed the
  skeletons, promote them with `git mv .heal/docs docs` and set
  `scaffold_root = "docs"` so the next regeneration writes
  directly into the published location.

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

## `[features.docs.todo_density]`

```toml
[features.docs.todo_density]
ignore_in_inline_code = true   # default: skip markers inside `…` spans
allowlist_paths       = []     # gitignore-style globs to skip entirely
```

`ignore_in_inline_code = true` (the default) keeps `TODO` /
`FIXME` / `XXX` / `TBD` / `[要確認]` / `[要修正]` mentions
*inside* single- or double-backtick spans from being counted.
Reference pages that document the marker keywords themselves (an
observer reference, a style guide explaining what `TODO` means)
are quoting the words rather than logging action items, so the
default opts those out without disabling the observer for the
project. Flip to `false` if your team uses inline-code spans for
real action items.

`allowlist_paths` skips matching docs entirely — useful when the
quoting pattern is the *whole* page and per-line stripping isn't
enough (e.g. a metric reference that lists every marker shape in
its body):

```toml
[features.docs.todo_density]
allowlist_paths = [
  "docs/reference/**/metrics.md",
]
```

Both knobs leave the count-to-Severity floors (3 = Medium, 10 =
High) untouched.

## `[features.docs.doc_link_health]`

```toml
[features.docs.doc_link_health]
exclude_link_prefixes = []   # default: check every internal link against the source tree
```

`exclude_link_prefixes` opts links whose target starts with any
listed prefix out of source-tree verification. The link is
counted as neither resolved nor broken — the resolver bypasses
it entirely. Use it for static-site deploy URLs that the
framework rewrites at build time:

```toml
[features.docs.doc_link_health]
exclude_link_prefixes = ["/heal/"]   # Starlight base: '/heal'
```

| Framework | Setting in framework config | `exclude_link_prefixes` value |
|-----------|----------------------------|-------------------------------|
| Astro Starlight | `base: '/heal'` | `["/heal/"]` |
| VitePress / Docusaurus | `base: '/docs/'` | `["/docs/"]` |
| mdBook | `book.url-prefix = "/guide"` | `["/guide/"]` |

The framework's own build-time link checker (e.g.
`astro build`) already validates these targets from the deploy
side, so heal can defer that slice without losing coverage.
Empty entries (`""`) are ignored — heal doesn't let a single
empty string accidentally silence the entire observer.

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
