# Wiki organization

How the emitted page set hangs together as a single navigable
artefact. The skill enforces this structure on every run; the
rationale is here so future edits don't drift.

## §1 Six-category top-level navigation

The wiki index emits exactly **six** top-level sections, in this
order:

```
Quick Start    → README, getting-started, system-context
Architecture   → overview, module-map, runtime-views, deployment,
                 bounded-contexts, data-model, crosscutting
Reference      → glossary, feature-catalog, api
Operations     → service-overview, runbooks, slos, postmortems,
                 oncall-onboarding
Decisions      → ADR index, ADRs, roadmap, risks
Contributing   → contributing, quality-goals, test-strategy,
                 security
```

Why six and not, say, ten? Two reasons:

1. **Cognitive load.** Readers can hold ~7±2 categories in
   working memory; six fits well under the cap.
2. **DeepWiki's empirical 30-page ceiling.** Six categories ×
   3–5 pages each = 18–30 pages, which matches the ceiling
   above which wikis become abandoned mass-archives instead of
   read tools.

The skill does **not** add a seventh top-level category.
Sub-sections within a category (e.g.
`Operations/Runbooks/<NNNN-NAME>`) are fine; another
top-level peer is not.

## §2 Filesystem layout

```
<repo root>/
├── README.md                                  # Tier 1 (root)
├── CONTRIBUTING.md                            # Optional — root
├── LICENSE                                    # Convention — not emitted
├── CHANGELOG.md                               # Convention — not emitted
└── <scaffold_root>/
    ├── index.md                               # Wiki entry
    ├── getting-started.md                     # Tier 1
    ├── contributing.md                        # Tier 2 (when no root one)
    ├── architecture/
    │   ├── system-context.md                  # Tier 1
    │   ├── overview.md                        # Tier 1
    │   ├── module-map.md                      # Tier 2
    │   ├── bounded-contexts.md                # Tier 3
    │   ├── runtime-views.md                   # Tier 3
    │   ├── data-model.md                      # Tier 3
    │   ├── deployment.md                      # Tier 3
    │   └── crosscutting.md                    # Tier 3
    ├── reference/
    │   ├── glossary.md                        # Tier 1
    │   ├── feature-catalog.md                 # Tier 2
    │   └── api.md                             # Tier 3
    ├── operations/                            # Tier 4 (entire dir
    │   ├── service-overview.md                #   only when service
    │   ├── slos.md                            #   detected or user
    │   ├── oncall-onboarding.md               #   opts in)
    │   ├── runbooks/
    │   │   ├── index.md
    │   │   └── 0000-template-sample.md
    │   └── postmortems/
    │       ├── index.md
    │       └── 0000-template.md
    ├── decisions/
    │   ├── index.md                           # Tier 2
    │   ├── 0000-template.md                   # Tier 2
    │   ├── roadmap.md                         # Tier 5
    │   └── risks.md                           # Tier 5
    └── strategy/
        ├── quality-goals.md                   # Tier 2
        ├── test-strategy.md                   # Tier 5
        └── security.md                        # Tier 5
```

The directory names map 1:1 to the index categories
(`architecture/` ↔ "Architecture", and so on). The two top-level
exceptions are `README.md` (GitHub repository convention,
front-of-the-house) and the optional root `CONTRIBUTING.md`
(also a GitHub convention — `contributing.md` under the
scaffold root duplicates the link target only when no root
file exists).

`LICENSE` and `CHANGELOG.md` are repository-root conventions
the skill **does not emit**. If they're missing, surface that
in the report; don't manufacture them.

## §3 Page-count discipline

The catalog totals 25 page types across five tiers:

```
Tier 1  Essential        4–5 pages
Tier 2  Recommended      5–7 pages
Tier 3  Domain-dependent 4–6 pages
Tier 4  Operational      4–5 pages
Tier 5  Strategic        3–5 pages
                         ──────────
                         20–28 pages, typical mature project
```

Multiply by audience size (one runbook per alert, one ADR per
decision, one postmortem per incident) and the **total** wiki —
including human-authored growth — converges around 30–80 pages
on real codebases. DeepWiki's product limits (30 pages free
tier, 80 pages enterprise) match this empirically.

The skill caps **scaffold-emitted** pages at the catalog.
User-authored growth past that is the user's choice; the skill
doesn't gate that. But if a user comes back asking "scaffold me
20 more pages", push back on the page-bloat anti-pattern
(`page-catalog.md` §5) before complying.

## §4 SSoT discipline

Every fact lives in exactly **one** page. Other pages link to
that page; they don't re-state.

| Fact | SSoT |
|---|---|
| Term definition | `reference/glossary.md` |
| Feature existence | `reference/feature-catalog.md` |
| Past decision rationale | The ADR for that decision |
| Current container layout | `architecture/overview.md` |
| Module responsibilities | `architecture/module-map.md` |
| API shape | Generator output (linked from `reference/api.md`) |
| Deployment topology | IaC files (linked from `architecture/deployment.md`) |
| Quality priorities | `strategy/quality-goals.md` |

When a fact is unstable enough that a single page can't be the
SSoT (e.g. "current production version"), the SSoT is the **code
or system that owns the fact**, not a doc. The doc links there.

The skill enforces this by:

- Pre-seeding `related_pages` cross-links instead of re-writing
  content across pages.
- Pointing `feature-catalog.md` at ADR-NNNN entries rather than
  embedding decision text into the catalog.
- Keeping the API page deliberately thin — the generated
  reference next to it carries the data.

## §5 Two-way links by default

Every cross-link is bidirectional. The closure is computed in
Phase 3 of the skill (per `SKILL.md`) and written to both pages'
`related_pages` frontmatter array. One-way links breed orphans:
when a referrer is deleted, the referent loses its only inbound
edge and slips off the navigable graph.

The exception is conventional entry points — `README.md` and
`<scaffold_root>/index.md`. They're reachable from outside the
doc graph (GitHub repo home, Starlight / mdBook home), so they
don't need incoming back-links.

## §6 Navigation rendering hints

The skill emits plain Markdown links inside `index.md` because
that's the lowest common denominator (every static-site
generator and the GitHub renderer both work). For projects that
already use Starlight / mdBook / Docusaurus / mkdocs, **do not
emit a `_sidebar.md` / `book.toml` / `docusaurus.config.js` /
`mkdocs.yml`** — those are project-specific configuration the
user already manages. The skill produces the **content**;
threading it into the site generator is the user's call.

When the user asks for it explicitly, the skill can emit a
DeepWiki-style steering file at `.devin/wiki.json` listing the
emitted pages. That file is **opt-in**, mentioned as a follow-up
in the report, never default.

## §7 Anti-patterns the layout enforces

These show up only when the layout is violated. Surface them in
the report so the user can spot drift.

1. **Code-mirror layout.** Don't lay out the wiki under
   `architecture/<package>/<module>.md`. The wiki structure is
   reader-question-shaped, not code-shaped — see
   `.claude/docs/doc-scaffold-design.md` §2.1 for the full
   argument.
2. **Deep nesting.** Three clicks from `index.md` to any page
   is the cap. The catalog tops out at 2 levels of depth
   (`<scaffold_root>/operations/runbooks/<NAME>.md`); more is
   over-engineering.
3. **Cross-tier mixing.** Don't put Tier 4 operational pages
   under `architecture/`. Each tier has a directory; keep
   them apart.
4. **Per-feature subdirectories.** A `features/payments/` tree
   with one page per feature explodes the Wiki. The Feature
   Catalog is **one** page; deep dives go into ADRs and
   Runtime Views.
5. **Auto-emitted `_sidebar.md` / generator config.** Site-
   generator configuration is the user's, not the skill's.
   Skill emits content; user wires it up.
