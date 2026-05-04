# Page catalog (5 tiers × 25 page types)

Per-page entry the scaffold skill consults when picking what to
emit. Each entry pins the **purpose**, the **must-include
sections**, the **AI-generation suitability**, and (where
applicable) the **human-only carve-out** marker.

The full literature lineage — Diátaxis × DeepWiki × arc42 × C4 ×
strategic DDD × ADR × SRE — lives in
`.claude/docs/doc-scaffold-design.md`. This file is the **operating
table**: short, decisive, scoped to what the skill needs at runtime.

## §1 Emit gate

The catalog's primary rule: **emit a page only when the codebase
can fill it with meaningful content.** Pages that would ship as
a stack of `TODO(human):` markers or `<not detected>` cells are
**not emitted**. Skeleton-only files are not the skill's output;
the user authors them when they have the relevant input.

That collapses what earlier drafts called the "human-only
carve-out" to a single artefact: the **ADR template**
(`decisions/0000-template.md`). It carries `TODO(human):`
markers in its Context / Decision / Consequences body sections,
because the template's purpose is "copy this file when filing
the next ADR" — the markers cue the writer at copy time. No
other page emitted by this skill carries `TODO(human):`.

| Page | Treatment |
|---|---|
| ADR template (`decisions/0000-template.md`) | Emit always, with `TODO(human):` markers inside Context / Decision / Consequences. |
| README "Why" section | Section omitted on auto-emit. User adds it when ready. |
| Individual ADR bodies | Authored from the template; not auto-generated. |
| Quality Goals, Bounded Context Map, Roadmap, Risks, Postmortems, Runbooks, Service Overview, SLOs, On-call onboarding, Security Posture | **Not emitted on first run.** Authored by the user when they have the input. |

The "skip rather than skeleton" rule is what the rest of this
catalog enforces.

## §2 The emit set

The catalog separates pages by **how the emit gate fires**.
Always-emit pages are the codebase-rich set every project has
real signal for. Conditional pages emit when their detection
trigger fires AND the auto-fill produces meaningful content.
Everything else stays unauthored until the user supplies input.

### Always-emit (universal codebase signal)

These six pages always have enough signal: every project has a
manifest, a primary entry point, and exported symbols.

| Page | Path | Auto-fill |
|---|---|---|
| README | `README.md` (root) | Project name (manifest), subtitle (manifest description), Quick Start (toolchain install + minimal run), Documentation map links, License. The "Why this exists" section is **omitted on auto-emit** — user adds when ready. |
| Wiki Index | `<scaffold_root>/index.md` | Six-category nav populated from the pages this run emits. |
| System Context | `<scaffold_root>/architecture/system-context.md` | Diagram with auto-detected actors (manifest deps, CI workflows, IaC). Boundaries paragraph from manifest + tree shape. |
| Architecture Overview | `<scaffold_root>/architecture/overview.md` | Container list from workspaces; Technology column (`Rust + clap` etc.) from manifest + detected framework. No rationale column — rationale belongs in ADRs. |
| Glossary | `<scaffold_root>/reference/glossary.md` | Rows pre-seeded from exported symbols + their docstrings. Skip rows where both symbol and module-level docs are silent. |
| Getting Started | `<scaffold_root>/getting-started.md` | Prerequisites + setup + test command from manifests; "First change" walk picked from low-risk file heuristics. |

### Conditional-emit (detection signal must be present)

Each page emits only when its detection trigger fires AND the
resulting page is mostly auto-fill, not mostly `<not detected>`.

| Page | Path | Emit when |
|---|---|---|
| Module Map | `<scaffold_root>/architecture/module-map.md` | More than one workspace / top-level module **and** at least one has a doc comment / module README / manifest description the skill can quote. |
| Feature Catalog | `<scaffold_root>/reference/feature-catalog.md` | CLI subcommands, HTTP route handlers, or README "Features" bullets parseable. Skip when the catalog would be empty. |
| ADR Index | `<scaffold_root>/decisions/index.md` | Always (the index is structural — numbering convention + status legend + template link). |
| ADR Template | `<scaffold_root>/decisions/0000-template.md` | Always (meaningful template — `TODO(human):` markers inside its body sections are intentional, by §1). |
| Contributing | `<scaffold_root>/contributing.md` (skip when root `CONTRIBUTING.md` exists) | At least two of: detected branch convention, PR template, CODEOWNERS, formatter / linter configs, CI workflow. |
| Runtime Views | `<scaffold_root>/architecture/runtime-views.md` | At least one entry-point cluster (CLI `main`, HTTP routes, queue consumers, top-level public function) detectable. |
| API Reference | `<scaffold_root>/reference/api.md` | OpenAPI / proto / GraphQL schema present anywhere in the tree. |
| Data Model | `<scaffold_root>/architecture/data-model.md` | Migration directory or schema definitions detected (`migrations/`, `db/migrate/`, `prisma/schema.prisma`, `*.sql` DDL, ORM model files). |
| Deployment View | `<scaffold_root>/architecture/deployment.md` | Deployment artefacts detected (`Dockerfile`, `docker-compose.yml`, `k8s/`, `helm/`, `terraform/`, `pulumi/`, `serverless.yml`). |
| Crosscutting Concepts | `<scaffold_root>/architecture/crosscutting.md` | At least three of the cross-cutting axes have detectable evidence (logging library, error pattern, auth middleware, transaction wrapper, i18n library, cache library). |
| Test Strategy | `<scaffold_root>/strategy/test-strategy.md` | `[features.test]` enabled in `.heal/config.toml`, OR a recognised test framework with > 10 detected tests. |

### Not emitted on first run

These are codebase-silent: the skill has no signal that would
let it fill them honestly, and emitting a skeleton would be
exactly the anti-pattern this catalog forbids. The user
authors them when they have the input — the skill stays out
of the way.

| Page | Why |
|---|---|
| Quality Goals & Constraints | Stakeholder priorities — organisational, not codebase-derived. |
| Bounded Context Map | Domain boundaries — organisational; even a detected DDD layered split doesn't yield context names or relationships. |
| Roadmap | Forward planning. |
| Risk & Tech-Debt Register | Empty on first run; user adds rows as items arise. |
| Service Overview | Owner, oncall, dashboards — all organisational. |
| SLO Documentation | SLI / SLO / Error budget — all organisational. |
| Runbook Index + sample | Runbooks land when alerts are configured; until then, the page is pure scaffolding. |
| Postmortem Index + template | Postmortems land when incidents occur. |
| On-call Onboarding | Access / contacts / page links are operational once a rotation exists. |
| Security Posture | Threat model is human-curated; first-run page would be 80%+ `<not detected>`. |

When the user authors one of these pages by hand, the existing-
file rule of the skill applies: the next regeneration leaves it
untouched.

## §3 Per-page metadata block

Every page carries one frontmatter field — `title`:

```yaml
---
title: <page title>
---
```

That's the whole block. Earlier drafts of the catalog also
emitted Diátaxis / audience / freshness owner / last review /
review cycle / related-pages / related-code / related-adrs.
Each was dropped because it was either recoverable from
`git log` (ownership, freshness) or duplicating information
already in the body (Diátaxis purpose is conveyed by the
content; "see also" Markdown links carry the cross-link graph;
`.heal/doc_pairs.json` carries doc ⇔ src mapping). See
`references/page-templates.md` §2 for the full table.

The minimal frontmatter has two consequences worth flagging:

- **No state-management drift on regenerate.** A re-run of the
  skill doesn't have to reconcile `last_review` dates or
  `freshness_owner` against the latest commit history.
- **No tooling lock-in.** Site generators that consume more
  than `title` (Starlight directive props, mkdocs-material
  metadata extensions) can be wired in by hand-editing the
  frontmatter — the skill's emit doesn't fight that edit.

## §4 Auto-fill summary (emitted pages only)

Each emitted page is auto-filled from detected codebase signal.
Cells the skill cannot fill honestly emit `<not detected>` —
never invented values. Pages whose §2 row sits under "Not
emitted on first run" do not appear in this table because they
are not produced.

| Page | What the skill writes |
|---|---|
| README | Project name, subtitle, install command, doc-map links, license. The "Why this exists" section is omitted (no `TODO(human):`). |
| Wiki Index | Nav structure pointing at every emitted page. |
| System Context | Diagram with detected actors and external systems; boundary paragraph from manifest metadata. |
| Architecture Overview | Container list from workspaces, tech (lang + framework) per row. Rationale moved to ADRs. |
| Glossary | Rows pre-seeded from exported symbols + their docstrings. |
| Getting Started | Prerequisites + commands from manifests; "first change" walk picked from low-risk file heuristics. |
| Module Map | Rows from detected packages; Responsibility from each module's top-level doc comment / module README. |
| Feature Catalog | Auto-fills from CLI subcommands / HTTP routes / README "Features" bullets. Skipped when the catalog would be empty. |
| ADR Index | Numbering + status legend + template link. No individual ADR is emitted. |
| ADR Template | Template file at `0000-template.md`. **The only emitted page that carries `TODO(human):`** — inside its Context / Decision / Consequences sections. |
| Contributing | Branch strategy from default branch + merge graph, PR template from `.github/`, review pattern from CODEOWNERS, formatters from configs, release pattern from CI. |
| Runtime View | One Mermaid sequence diagram per detected entry-point cluster, with prose from the entry function's doc comment. |
| API Reference | Schema link + generator hookup; auth / rate-limits / error model parsed from schema. |
| Data Model | ER from migrations + per-entity sections from model docstrings. |
| Deployment View | Env list from IaC; topology from compose / k8s / Helm; secrets approach from detected references. |
| Crosscutting | Per-section content from logging library, error pattern, auth middleware, i18n / cache / transaction signals. |
| Test Strategy | Pyramid shape from test counts + per-layer framework from manifests + lcov path from `[features.test.coverage]`. |

Across the whole emit set, `TODO(human):` appears in exactly
one file — the ADR template.

## §5 Anti-patterns the skill must refuse

These are the failure modes that show up when scaffold tools
over-reach. Hard-coded into the skill body so they survive future
edits.

1. **Skeleton-only pages.** A page that ships as a stack of
   `TODO(human):` markers or `<not detected>` cells is not
   emitted. The user authors it when they have the input. The
   only `TODO(human):` markers in this skill's output are inside
   the ADR template's body sections — that template is meaningful
   (a copy-source for filing the next ADR), not a skeleton.
2. **Manufacturing prose to clear `doc_coverage`.** A one-line
   stub like `# CLI\n\nCLI documentation.\n` is forbidden — the
   Coverage trap. Skip the page instead.
3. **Mirroring `src/` into `<scaffold_root>/`.** Wiki structure
   is question-shaped, not code-shaped — the page plan reflects
   reader questions (`.claude/docs/doc-scaffold-design.md` §2.1).
4. **Inventing values.** Owner names, oncall rotations,
   dashboard URLs, SLO numbers, threat-model claims — never
   guessed. The honest options are auto-fill from a detected
   source or skip the page.
5. **Emitting > 30 pages.** The page plan caps the catalog at
   ~28 page types; user-authored growth past that is the user's
   call.
6. **Deleting human-authored files.** Even with `--force`. The
   contract is "overwrite generated pages"; "delete arbitrary
   files in the doc tree" is out of scope.
7. **Auto-translating.** When the project's existing prose is
   non-English, the skill emits English content and surfaces a
   note in the summary. The user picks the locale strategy.
