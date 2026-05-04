# Doc-scaffold design rationale

Internal background for the `/heal-doc-scaffold` skill. Captures
the literature lineage, the reader-question framework, the
AI-generation tradeoffs, and the anti-pattern catalogue that
shapes the page catalog. AI agents working on the skill (or on
adjacent docs observers) consult this doc; the skill itself
references only the operating subset under
`crates/cli/skills/heal-doc-scaffold/references/`.

## §1 Literature lineage

The page catalog merges seven independent traditions. Each one
solves a different problem; the scaffold's value is the
intersection.

### §1.1 Diátaxis (Procida)

Splits docs by **purpose** into Tutorial / How-to / Reference /
Explanation. Diátaxis is an *axis*, not a page list. The
scaffold catalog uses it to **shape each page** (the body's
voice, structure, depth) so the right quality bar applies.
Reference rigor on an Explanation page over-fires the
`[features.docs]` observers; Tutorial laxity on a Reference
page under-fires them. The classification is conveyed by the
body's content, not a frontmatter tag — see §10 for why the
scaffold drops `diataxis: …` from emitted frontmatter.

### §1.2 README culture

GitHub-era de-facto standard. `makeareadme.com`, GDS, freeCodeCamp
all converge on the same five-section shape: what is this / why
does it exist / how to install / how to contribute / license. The
scaffold's `README.md` template is this shape, with the "Why"
section hard-coded as the human-only carve-out (it cannot be
auto-derived from code).

### §1.3 arc42 (Starke & Hruschka)

12-section "cabinet" template for software architecture. Not
"fill all 12" — "fill the drawers that matter." The scaffold's
Tier 1–3 architecture pages map directly to arc42 sections:
Introduction & Goals → Quality Goals; Context & Scope → System
Context; Building Block View → Module Map / Architecture
Overview; Runtime View → Runtime Views; Deployment View →
Deployment; Crosscutting → Crosscutting; Decisions → ADR Index;
Glossary → Glossary; Quality Requirements + Risk → Quality Goals
+ Risks Register.

### §1.4 C4 model (Brown)

Four levels of architectural zoom: System Context → Container →
Component → Code. Level 4 is dead on arrival (auto-generated, can
never be hand-maintained). The scaffold emits Level 1 (System
Context) and Level 2 (Container — what the catalog calls
"Architecture Overview"), Level 3 (Components — "Module Map") on
demand. Level 4 is left to whatever generator the project uses.

### §1.5 Strategic DDD (Evans, Vernon)

Bounded Context + Context Map vocabulary for domain-shaped
applications. The 9-pattern relationship list (Partnership,
Shared Kernel, Customer-Supplier, Conformist, Anti-Corruption
Layer, Open Host Service, Published Language, Separate Ways,
Big Ball of Mud) is the scaffold's Bounded Context page. DDD
vocabulary is **wrong** for codebases without a domain (CLI
tools, libraries, infrastructure code) — the scaffold detects
the layered split heuristic before suggesting the page.

### §1.6 ADR (Nygard)

Title / Status / Context / Decision / Consequences in five
short sections, frozen on accept, never edited after that.
MADR adds Markdown formatting conventions. The scaffold emits
the ADR Index + the template; never an individual ADR (those are
human-only by §4 below).

### §1.7 SRE (Google)

Service Overview / Production Readiness Review, Runbooks,
Postmortems, SLOs. Operational pages exist only when the project
has production deployment surface; the scaffold gates Tier 4
behind explicit detection of deployment artefacts.

### §1.8 DeepWiki (Cognition AI)

Empirical regularities of AI-generated wikis: ≤ 30 pages free,
≤ 80 enterprise; two-level hierarchy works, deeper is over-
engineered; structure-from-code is auto-generatable, judgment
is not. The scaffold matches the 30-page ceiling (28 page types
across all tiers) and uses the two-level layout.

## §2 Reader-Question Matrix

The catalog is shaped by **reader × question**, not by code
structure. The matrix below is the operating data:

| Reader | Question | Page that answers |
|---|---|---|
| `new-1d` | What is this? Where do I start? | README, Getting Started, System Context |
| `new-1w` | How is the codebase laid out? | Architecture Overview, Module Map |
| `feature-owner` | What does feature X do? Where? | Feature Catalog, API Reference |
| `debugger` | Why does it behave this way? | Runtime Views, ADR Index |
| `reviewer` | Is this PR aligned with priorities? | ADR Index, Quality Goals |
| `oncall` | This alert fired. What do I do? | Runbook, Service Overview |
| `ops` | Where is this deployed? What's the SLO? | Deployment View, SLOs |
| `product` | What ships? When? | Feature Catalog, Roadmap |
| `external-integrator` | API? Schema? Auth? | API Reference, Crosscutting |
| `legal-license` | What's included? Dependencies? Licenses? | LICENSE, root manifests |

Two principles fall out:

1. **A reader without a page is a gap.** When the matrix has a
   row whose page column is blank for this project, the wiki
   has a missing slot.
2. **A page without a reader is bloat.** Every page has at
   least one row claiming it. Page types that never appear in
   the matrix don't enter the catalog. (`orphan_pages` measures
   this in the running observable system; the scaffold pre-
   prevents it by only emitting page types that have readers.)

Earlier drafts of this catalog tagged each emitted page with an
`audience: [...]` frontmatter array. That has been dropped (see
§10) — the body's content already speaks to the audience, and
maintaining a tag in frontmatter alongside the prose was
state-management duplication. The matrix above stays as the
**internal design rule** for what page types make the cut, not
a runtime classification.

## §3 Tier rationale

Why five tiers and not three or seven?

- **Tier 1 (essential)** — every project, including a one-file
  utility. README + glossary + getting started is the minimum
  viable wiki.
- **Tier 2 (recommended)** — past the toy stage. ADR Index +
  Feature Catalog + Quality Goals only pay off when there are
  multiple decisions and multiple features. A 50-LOC utility
  doesn't have those.
- **Tier 3 (domain-dependent)** — applies only when the
  codebase has the relevant shape. DDD pages are wrong on a CLI
  tool; API Reference is wrong on a library with no exposed
  surface; Data Model is wrong on a stateless transformer.
- **Tier 4 (operational)** — applies only when the project
  ships to production. Library projects don't have runbooks.
- **Tier 5 (strategic)** — discretionary. A short-lived
  prototype doesn't need a Roadmap or a Risk Register.

The tiering is **fail-soft**: a project that doesn't fit a tier
skips it cleanly, leaving a smaller but coherent wiki. The
alternative — a fixed 25-page template applied to every project
— produces ghost pages on small projects and inadequate
coverage on large ones.

## §4 No skeleton-only pages — strict emit gate

Earlier drafts of this design described a "human-only carve-
out" where six page kinds emitted as `TODO(human):` skeletons.
That was the wrong shape. The current rule is stronger and
simpler:

> A page emits only when the codebase can fill it with
> meaningful content. Pages that would ship as a stack of
> `TODO(human):` markers or `<not detected>` cells are **not
> emitted on first run**. The user authors them when they have
> the input.

The previous carve-out list collapses to two distinct things:

1. **Pages not emitted at all on first run** — Quality Goals,
   Bounded Context Map, Roadmap, Risks Register, Service
   Overview, SLO Doc, Runbooks, Postmortems, On-call
   Onboarding, Security Posture. The skill leaves them absent
   until the user has the input.
2. **The ADR template** (`decisions/0000-template.md`). This
   is the **only** emitted file that carries `TODO(human):`
   markers — they appear inside the template's Context /
   Decision / Consequences body sections, where they cue the
   writer when they copy the template to file the next ADR.
   The template itself is meaningful (a real copy-source), not
   a skeleton.

Why no skeleton-only pages?

| Page kind | Why human-only — and why "skip not skeleton" beats "TODO skeleton" |
|---|---|
| README "Why" section | Project motivation lives in the maintainer's head — it cannot be reconstructed from code. Mis-describing it is worse than leaving it blank. **The auto-emitted README simply omits this section** — when the user adds it, missing-only / reconcile preserves their addition. |
| Individual ADR body | An ADR records judgment **at the moment of decision**. The reasons applied at time T cannot be reproduced at time T+1 from code that's been modified since. Auto-filling reads as historical revisionism. **No individual ADR is emitted** — the Index + Template are scaffolding the user copies from. |
| Quality Goals & Constraints | Stakeholder-negotiated priority list. The trade-offs are organisational, not technical. |
| Bounded Context Map | Domain boundaries are organisational and political. The skill's "layered split detected" heuristic recognises **shape**, not **meaning**. |
| Roadmap | Forward planning. Cannot be derived from past code. |
| Postmortem "Lessons" section | Insight from experiencing an incident. The data is the timeline; the **lesson** is what the people involved learned. |

Auto-filling these — even with apparently-plausible prose —
poisons the page: the writer who arrives later cannot tell what
is real from what is filler, and the natural response is to
delete the whole page rather than edit each line. The previous
"emit a `TODO(human):` skeleton" rule had its own failure mode:
the file ships as a stack of TODOs that the team learns to skip,
which drives the noise floor up across the wiki.

The current rule — *don't emit the page at all* — keeps the
generated wiki dense with auto-fillable substance. When the
team is ready to add a Quality Goals page (or a Roadmap, or a
Service Overview), they create the file by hand. The skill's
default reconcile mode preserves it on every subsequent re-run.

The `[features.docs]` `todo_density` observer still recognises
the `TODO(human):` marker — but the only emitted file that
carries it is the ADR template, where the markers are
intentional (cues at copy time). They don't surface as findings
because the template is one file, not a tree of skeletons.

## §5 SSoT discipline

The catalog is built on the principle "**every fact lives in
exactly one page**." This is the only way to keep the wiki's
`duplication` finding count low without manual policing.

The SSoT table:

| Fact | SSoT page | Why |
|---|---|---|
| Term definition | `glossary.md` | Single Ubiquitous Language |
| Feature existence | `feature-catalog.md` | Product-developer interface |
| Past decision rationale | The ADR for the decision | Frozen at decision time |
| Current container layout | `architecture/overview.md` | One picture, not many |
| Module responsibilities | `architecture/module-map.md` | Static structure |
| API shape | Generator output | Code is canonical |
| Deployment topology | IaC files | Code is canonical |
| Quality priorities | `strategy/quality-goals.md` | Tiebreaker reference |
| Test policy | `strategy/test-strategy.md` | Layered policy |
| Security posture | `strategy/security.md` | Single threat model |

The skill enforces SSoT by:

1. **Computing cross-links once, writing both ends.** Phase 3
   of the skill (per `SKILL.md`) emits "See also" Markdown
   links on both referrer and referent.
2. **Pointing instead of duplicating.** `feature-catalog.md`
   rows reference ADR-NNNN entries by number; they don't quote
   the ADR text.
3. **Refusing to embed.** When a writer asks "should I copy
   this paragraph from the ADR into the Feature Catalog?", the
   answer is "link, don't copy" — the catalog row carries one
   sentence and a link, not a précis.

## §6 AI-generation tradeoffs

Per DeepWiki's experience and the catalog's own analysis, page
types differ sharply in how well an AI can produce them.

### §6.1 The full table

| Page kind | AI fit | Reason |
|---|---|---|
| Module Map, API Reference (generator) | full | Structural information lives entirely in code |
| Wiki Index | full | Mechanical from the page list |
| ADR Template, Postmortem Template | full | The shape is fixed; specific content is `TODO(human):` |
| Architecture Overview (containers) | partial | Auto-derive container list; rationale is human |
| System Context (skeleton) | partial | Skeleton + diagram fence; actors are human |
| Glossary (seed) | partial | Pre-seed rows from exports; definitions are human |
| Module Map (rows) | partial | Auto-derive workspaces; responsibility is human |
| Getting Started | partial | Auto-derive commands; "first change" walk is human |
| Crosscutting (sections) | partial | Section headers auto; policy per section is human |
| Data Model (ER) | partial | ER from migrations; invariants are human |
| Deployment View (envs) | partial | Env list from IaC; network paths are human |
| Runbook (template) | partial | Template structure auto; specifics are human |
| Postmortem (timeline) | partial | Timeline from incident logs; **Lessons is human-only** |
| Test Strategy | partial | Pyramid headers auto; per-layer policy is human |
| Bounded Context Map | none | Domain boundaries are organisational |
| ADR (individual) | none | Frozen judgment at decision time |
| Quality Goals | none | Stakeholder-negotiated |
| Roadmap | none | Forward planning |
| Service Overview | none | Owner / oncall / SLO are human |
| SLO Doc | none | Targets are human-negotiated |
| Security Posture | none | Threat model is human-curated |
| README "Why" section | none | Motivation is human |

### §6.2 The minimum human investment

A team can run the scaffold and then focus its writing energy on
the **none** column above. Specifically:

1. README "Why" — one paragraph
2. Each ADR body (as decisions land) — five short sections
3. Quality Goals — three rows
4. Bounded Context Map (if applicable) — one diagram + one
   section per context
5. Service Overview + SLO Doc + Security Posture (if Tier 4 /
   Tier 5 applies) — table cells
6. Postmortem "Lessons" sections (per incident) — short
   paragraphs

Everything else — structural pages, table rows, diagram
skeletons, indexes, templates — the skill produces. The team's
writing budget concentrates on the irreducible-judgment subset
the §4 list spells out.

### §6.3 What the AI must refuse

When asked to fill a §4 page despite the carve-out, the skill
declines with a one-line explanation. The user can hand-write
the page, of course; what's forbidden is **the skill** producing
plausible-sounding content for these page kinds.

The reason this matters: a page that *looks* filled but
*reads* as generic is harder to fix than a skeleton. A reader
who sees `TODO(human):` knows the page is incomplete; a reader
who sees "This project solves modern challenges with elegant
solutions" thinks the page is complete and stops engaging with
it. The latter erodes trust in the entire wiki.

## §7 Anti-patterns the catalog refuses

The full list lives in `references/page-catalog.md` §5 and
`references/wiki-organization.md` §7. The summary:

1. **Code-mirror layout** — `<scaffold_root>/<package>/<module>.md`
   matches code structure but doesn't answer reader questions.
2. **Page bloat** — emitting pages "just in case." The catalog
   caps at 28 page types; user-authored growth is fine but
   should be deliberate.
3. **Tutorial / Reference mixing** — Diátaxis-violating pages
   serve neither audience.
4. **ADR retro-fitting** — an ADR written months after the
   decision is a lie. The skill emits the template only.
5. **Empty docstrings to clear `doc_coverage`** — the Coverage
   trap. The skill never produces a stub-only page.
6. **External-link perfectionism** — chasing zero-broken-link
   metrics removes useful citations. Link rot is real but the
   answer is archive snapshots, not censorship.
7. **One-way cross-links** — backed by the orphan-pages
   observer. Every link must be bidirectional unless the
   referrer is a conventional entry point.
8. **`features/<feature>/<page>.md` per-feature subdirectories**
   — explodes the Wiki. Feature Catalog is *one* page; deep
   dives are ADRs and Runtime Views.
9. **Auto-emitting site-generator configuration** — `_sidebar.md`,
   `book.toml`, `mkdocs.yml`, `astro.config.mjs` are project
   choices the user already manages. The skill writes content,
   not config.
10. **Auto-translation** — when the project's docs are
    non-English, the skill emits English skeletons and notes
    the locale split. The user picks the strategy.

## §8 Autonomy contract (why the emit shape is what it is)

Two design choices shape the user experience and deserve their
own argument:

### §8.1 No `AskUserQuestion` for tier selection

The skill decides tiers from detection signals alone:

- Tier 1, 2: always emit.
- Tier 3: per-page, gated by detection signals (DDD split, API
  schema present, migrations present, IaC present).
- Tier 4: gated on **any** deployment artefact; emit the full
  set when triggered, skip the entire tier otherwise.
- Tier 5: always emit.

The earlier draft used `multiSelect` `AskUserQuestion` calls
for Tier 3 / 4 / 5 selection. That was wrong for two reasons:

1. **Menu interaction is more friction than file review.** A
   single review pass over a generated tree (delete what
   doesn't apply) is faster than three menu prompts where the
   user has to predict applicability before seeing the output.
2. **Detection signals are usually decisive.** A repo with a
   Dockerfile + Helm chart + k8s manifests obviously needs
   Tier 4. A repo with `Cargo.toml [lib]` only and no
   deployment artefacts obviously doesn't. The intermediate
   case (library + Dockerfile-for-CI-only) is rare; a
   single `AskUserQuestion` is the escape hatch when the
   skill genuinely can't tell.

The autonomy contract is: **detection-driven by default, ask
only when ambiguity is real.** Calls to `AskUserQuestion` from
this skill should be the exception, not the path.

### §8.2 Minimal frontmatter (`title:` only)

Earlier drafts emitted a 9-field frontmatter block:

```yaml
---
title: ...
diataxis: tutorial | how-to | reference | explanation
audience: [...]
freshness_owner: ...
last_review: ...
review_cycle: ...
related_pages: [...]
related_code: [...]
related_adrs: [...]
---
```

Eight of those nine fields have been dropped. The fields fall
into two categories, both of which the dropped form serves
poorly:

- **State-management fields** (`freshness_owner`,
  `last_review`, `review_cycle`): re-derivable from
  `git log --format='%aN %cI' -- <path>`. Carrying a
  duplicate copy in frontmatter creates drift the moment
  someone edits the file without updating the frontmatter
  date. The `[features.docs]` `doc_freshness` observer
  computes commit distance directly from git, never from
  frontmatter.
- **Classification / cross-link fields** (`diataxis`,
  `audience`, `related_pages`, `related_code`,
  `related_adrs`): redundant with body content. Diátaxis
  purpose is conveyed by the body's voice and structure.
  Audience is signalled by the same. Cross-links live in
  body Markdown links and `## See also` sections (which the
  `orphan_pages` observer reads anyway). The `related_code`
  field duplicated `.heal/doc_pairs.json`; the `related_adrs`
  field duplicated body Markdown links.

What's left:

```yaml
---
title: <page title>
---
```

`title` survives because Starlight / mkdocs-material / Docusaurus
sidebars key on it for nav rendering. mdBook can derive from
H1, but the broader site-generator ecosystem can't.

The `[features.docs]` observer family does **not** consume
frontmatter; it walks bodies and `git log`. The frontmatter
choice is purely about site-generator interop.

### §8.3 `TODO(human):` is rare by construction

After the §4 rewrite, the only emitted file carrying
`TODO(human):` markers is the ADR template
(`decisions/0000-template.md`). The marker is a copy-time cue
("when you file your next ADR, fill these sections"), not a
to-do at the wiki's table of contents. Pages that earlier
drafts emitted as TODO skeletons (Quality Goals, Bounded
Context Map, Roadmap, Postmortem Lessons section) are now
*absent* from first-run output instead. When a reader sees
`TODO(human):`, they're inside the ADR template — a known,
single, deliberate place.

### §8.4 Idempotent re-run (the contract)

The skill is **safe to invoke any number of times**. The
five-phase pipeline (Detect codebase → Survey existing tree →
Reconcile → Emit → Report) makes re-runs first-class:

1. **First run on an empty tree:** emits the codebase-derived
   pages.
2. **Second run after the user authored some pages:** the
   user's pages are preserved untouched (those paths are not
   in the auto-managed shape; Phase 2 classifies them as
   hand-authored).
3. **Third run after the codebase changed:** auto-managed
   sections inside the auto-emitted pages refresh (new
   container added → Architecture Overview's table grows; new
   exported symbol → Glossary gets a new row); hand-edits
   inside those same pages survive.
4. **Re-run after the user deleted an emit-set page:** the
   skill re-emits it on the next run (the existing-tree gate
   sees the missing file and treats it like a first run for
   that page).

The reconcile heuristic is fuzzy by design — Phase 2 reads
each existing section and asks "does this still match the
template's auto-fill shape, or has the team taken ownership of
it?" The answer drives whether Phase 4 refreshes or preserves
the section. When in doubt, the skill preserves: the cost of
leaving a section stale is small (next run catches up), the
cost of stomping a hand-edit is high.

Two flags override the default:

- `--missing-only`: skip Phase 2 entirely; emit only files
  that don't exist. Useful when the user wants the skill to
  act as an additive bootstrap (don't touch anything I've
  already written).
- `--force`: skip Phase 2's per-section classification; treat
  every emit-set page as fully auto-managed. This **does**
  overwrite hand-edits in those pages — explicit user choice.
  Files outside the emit set remain sacred even under
  `--force`.

The contract this whole section comes back to is one
sentence: **the skill never loses user work**. The default
mode is the strictest version of that — preserve hand-edits
section-by-section. The flags relax the contract only in the
direction the user explicitly chose.

## §9 Forward-looking notes (out of scope today)

Not implemented in v0.4; mentioned so future work doesn't
reinvent the framing.

### §9.1 DeepWiki sidecar (`.devin/wiki.json`)

Optional output mode where the skill emits a steering file
listing the emitted pages with their purpose and parent. This
makes DeepWiki / Cursor docgen / similar tools generate higher-
quality content within the same structure. Implementation cost:
~30 lines of JSON serialisation. Not default — the file ties the
project to a specific tool family.

### §9.2 Locale-aware emit

Detect the project's primary doc language from existing prose
(`docs/index.md` text, `README.md` text), prompt the user to
either continue in English or pick a target locale. The
mechanical pieces (frontmatter keys, `TODO(human):` markers)
remain in the source language; only the prose templates would
shift.

### §9.3 Live regeneration on commit

A future post-commit hook could detect "the `<scaffold_root>/`
metadata `last_review` field is N months old AND the paired
src changed K times since" and surface a `doc_freshness`
finding for the scaffold metadata itself. This is an
extension of the existing `doc_freshness` observer, not new
machinery. v0.5+ candidate.

### §9.4 Diátaxis-violation detection

Static analysis pass over scaffold output (and user-authored
edits) to detect Diátaxis-mixing pages — e.g. a "Reference"
page with imperative second-person prose, or a "Tutorial" page
with no concrete commands. Could plug into the existing docs
observer family as a new metric. v0.5+ candidate, probably
gated on prompt-engineered classifier rather than rule-based.

## §10 References

### §10.1 Doc structure literature

- Procida, D. *Diátaxis*. <https://diataxis.fr/>
- Starke, G. & Hruschka, P. *arc42 by example*, 2nd ed. 2017.
  <https://arc42.org>
- Brown, S. *Software Architecture for Developers*. Leanpub, 2015.
  <https://c4model.com>
- Brown, S. *The C4 Model: Visualizing Software Architecture*.
  O'Reilly, 2025.
- Knuth, D. E. "Literate Programming." *The Computer Journal*, 1984.
- Martraire, C. *Living Documentation*. Addison-Wesley, 2018.

### §10.2 Domain-Driven Design

- Evans, E. *Domain-Driven Design*. Addison-Wesley, 2003.
- Vernon, V. *Implementing Domain-Driven Design*. Addison-
  Wesley, 2013.
- Brandolini, A. "Strategic Domain Driven Design with Context
  Mapping." *InfoQ*, 2009.
- ddd-crew. *Context Mapping Cheat Sheet*.
  <https://github.com/ddd-crew/context-mapping>

### §10.3 Architecture Decision Records

- Nygard, M. "Documenting Architecture Decisions." *Cognitect
  blog*, 2011.
- ADR Organization. <https://adr.github.io>
- MADR. <https://adr.github.io/madr/>
- Keeling, M. "Love Unrequited: The Story of Architecture, Agile,
  and How Architecture Decision Records Brought Them Together."
  *IEEE Software*, 2022.

### §10.4 SRE

- Beyer, B. et al. *Site Reliability Engineering*. O'Reilly, 2016.
- Beyer, B. et al. *The Site Reliability Workbook*. O'Reilly, 2018.
- Treynor, B. et al. "Why SRE Documents Matter." *ACM Queue*, 2018.

### §10.5 README & community

- *Make a README*. <https://www.makeareadme.com/>
- GitHub. *About READMEs*. <https://docs.github.com/>
- GDS. *Documenting your code*.
  <https://gds-way.digital.cabinet-office.gov.uk/>

### §10.6 AI-generated wikis

- Cognition AI. *DeepWiki*. <https://deepwiki.com>
- Cognition AI. *DeepWiki documentation*.
  <https://docs.devin.ai/work-with-devin/deepwiki>

### §10.7 In-tree references

- `.claude/docs/observers.md` — `[features.docs]` observer
  details that drive `doc_freshness` / `doc_drift` /
  `doc_coverage` / `doc_link_health` / `orphan_pages` /
  `todo_density` findings on emitted skeletons.
- `.claude/docs/data-model.md` — `DocsConfig` field reference
  (`scaffold_root`, `pairs_path`, etc.).
- `crates/cli/skills/heal-doc-scaffold/references/page-catalog.md`
  — operating subset the skill consults at runtime.
- `crates/cli/skills/heal-doc-scaffold/references/page-templates.md`
  — body skeletons.
- `crates/cli/skills/heal-doc-scaffold/references/wiki-organization.md`
  — filesystem layout and navigation rules.
