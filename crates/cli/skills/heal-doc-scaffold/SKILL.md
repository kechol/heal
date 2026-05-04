---
name: heal-doc-scaffold
description: Stand up a project's documentation tree from scratch — autonomously, from codebase signals alone (no `AskUserQuestion` calls). Emits **only** pages the codebase can fill meaningfully: container lists, module responsibilities, glossary seeds, getting-started commands, API references, ER tables, runtime sequence diagrams, contributing rules — all derived from manifests, source comments, IaC, and CI configs. Pages whose value comes from organisational decisions (Quality Goals, Bounded Context Map, Roadmap, Service Overview, SLOs, Runbooks, Postmortems, Security Posture) are **not generated as skeletons** — they're skipped on first run and added when the user has the relevant input. `TODO(human):` markers are reserved for the absolute minimum: ADR bodies (frozen judgment) and Postmortem "Lessons" sections, both inside templates that only emit when the user actually files an ADR or postmortem. Output lands under `[features.docs] scaffold_root` (default `.heal/docs/`). Missing-only by default; `--force` overwrites. Trigger on "scaffold the docs tree", "generate the wiki", "build the documentation from scratch", "/heal-doc-scaffold".
---

# heal-doc-scaffold

One-shot skill that produces (or extends) the project's documentation
tree. Works in four phases: **detect** the project shape, **decide**
which Tier-1 → Tier-5 pages apply, **emit** Markdown skeletons with
frontmatter + `TODO(human):` markers under
`[features.docs] scaffold_root`, and **report** what was created vs
skipped. The HEAL binary never reads or writes the doc tree itself;
this skill is the only producer.

This is the **bootstrap** side of doc maintenance. The audit /
prioritize side is `/heal-doc-review`; the mechanical fix loop is
`/heal-doc-patch`. None of those skills can stand up a tree from
nothing — that's why this skill exists.

## Mental model

The reference page set comes from §3 of
`references/page-catalog.md` (Diátaxis × DeepWiki × arc42 × C4 ×
strategic DDD × ADR × SRE). The catalog organises 25 page types
into five tiers:

```
Tier 1 (4–5 pages)   Essential   — README, System Context,
                                   Architecture Overview, Glossary,
                                   Getting Started
Tier 2 (5–7 pages)   Recommended — Module Map, Feature Catalog,
                                   ADR Index, Contributing,
                                   Quality Goals
Tier 3 (4–6 pages)   Domain-     — Bounded Context Map, API Ref,
                     dependent     Runtime View, Data Model,
                                   Deployment, Crosscutting
Tier 4 (4–5 pages)   Operational — Service Overview, Runbooks,
                                   SLOs, Postmortems, On-call
Tier 5 (3–5 pages)   Strategic   — Roadmap, Risk Register,
                                   Test Strategy, Security
```

A typical mature codebase wants 20–28 of these pages — not 200.
Page-count discipline is part of the contract; see
`references/wiki-organization.md` §3 for why DeepWiki's 30-page
ceiling is approximately right.

The skill emits a **codebase-derived** wiki: a page lands only
when there's enough signal to fill it with real content.
Skeleton-only pages — those that would ship as a stack of
`TODO(human):` markers — are simply not generated. The user
adds them later when they have the relevant input (a real
incident produces the first postmortem; a chosen Roadmap drives
the Roadmap page; an organisational priority list drives Quality
Goals). The wiki on day 1 is smaller than the catalog's full
range, on purpose.

The only `TODO(human):` markers that ever ship are inside the
ADR template and the Postmortem template — and those templates
are themselves emitted only when an actual ADR / postmortem is
filed (the skill emits the Index page and the template file;
individual entries are user-authored). Everywhere else the
output is auto-filled content or `<not detected>` — never a
placeholder calling for the writer to draft a section.

## When this skill is right

- Brand-new project that has zero documentation and the user wants
  a starting structure (not "1 README", but the full tier-appropriate
  skeleton).
- Long-running project where docs grew ad-hoc, the user wants a
  consistent baseline, and missing-only mode can fill the gaps
  without overwriting human work.
- After enabling `[features.docs]` and running
  `/heal-doc-pair-setup`, the team realises half the pairs point at
  pages that don't exist yet — scaffold them so `doc_coverage`
  findings flow.

If the user wants to **review** existing doc health, use
`/heal-doc-review`. If they want to **fix** specific findings,
use `/heal-doc-patch`. If they want to **map** docs ↔ src, use
`/heal-doc-pair-setup`.

## References (load on demand)

- `references/page-catalog.md` — the 25 page types organised into
  five tiers, each with: Diátaxis purpose, must-include sections,
  audience, AI-generation suitability, and the human-only carve-out
  list (`references/page-catalog.md` §1).
- `references/page-templates.md` — the body skeletons, frontmatter
  schema, and `TODO(human):` placement rules per page.
- `references/wiki-organization.md` — filesystem layout, navigation
  pattern (six-category top), SSoT discipline, anti-patterns.

## Pre-flight (refuse to start when these fail)

1. **Configured root.** Read
   `[features.docs] scaffold_root` from `.heal/config.toml`. The
   default is `.heal/docs/`, which keeps scaffold output under
   the `.heal/` umbrella alongside `config.toml`,
   `calibration.toml`, and `doc_pairs.json`. That default
   intentionally avoids colliding with any existing `docs/` tree
   the project owns (Starlight, mdBook, mkdocs). Once the user
   reviews the skeletons they typically `git mv .heal/docs docs`
   and set `scaffold_root = "docs"` so the next regeneration
   lands directly in the published location. Whatever value
   `scaffold_root` has on this run is the only tree the skill
   writes to.
2. **`[features.docs]` awareness.** Probe with
   `heal status --feature docs --json`. If the docs family is off,
   warn that the skeleton's frontmatter is consumer metadata for
   that family — nothing is broken, but the observers won't see
   the new pages until the family is enabled. Continue (this skill
   doesn't depend on the family being on).
3. **Existing tree mode — reconcile by default.** Inspect
   `<scaffold_root>/`:
   - **Empty / missing:** create the tree from scratch.
   - **Non-empty:** the skill is **safe to re-run**. Do not
     skip-everything by default; instead, **reconcile** (Phase
     2 + Phase 3 below) so freshened codebase signal flows
     into auto-managed sections without disturbing the user's
     hand-edits.
   - `--missing-only` flag: only emit pages that don't exist;
     leave every existing file untouched. Use when the user
     wants the skill to act as an "additive bootstrap" only.
   - `--force` flag: regenerate all emit-set pages from
     scratch, overriding hand-edits. Use this knowingly — it
     is destructive of user work in those pages. The skill
     still **never** touches files outside the emit set, even
     under `--force`.
   - In all modes, files **outside the emit set** (whatever
     the user authored on their own — Quality Goals, Roadmap,
     filed ADRs, runbooks, postmortems, etc.) are sacred. The
     skill never deletes or rewrites them.
4. **Clean worktree (recommended).** Not enforced — running on a
   dirty tree is fine — but call out in the summary if there were
   uncommitted changes when the skill ran, since the diff a
   reviewer sees will mix unrelated work with scaffold output.

## Procedure (Detect → Survey → Reconcile → Emit → Report)

The five-phase shape is what makes the skill **safe to re-run**.
First-run and Nth-run go through the same pipeline; the
existing tree just becomes input that Phase 2 + Phase 3 reason
about.

### Phase 1 — Detect (codebase)

Survey the project so the page plan reflects what's actually here.
The detector reads, never writes.

| Signal | What it tells the page plan |
|---|---|
| `Cargo.toml` / `package.json` / `pyproject.toml` / `go.mod` / `build.sbt` | Primary language & toolchain (drives Getting Started commands) |
| Workspace layout (`crates/*`, `apps/*`, `pkg/*`, `services/*`) | Multi-package → Module Map needs explicit cluster section |
| `Dockerfile`, `docker-compose.yml`, `k8s/`, `helm/`, `terraform/`, `pulumi/` | Deployable service → Tier 4 candidate (Service Overview / Runbooks / SLOs) |
| `.github/workflows/`, `.circleci/`, `.gitlab-ci/` | CI exists → Contributing must reference the actual workflow |
| `controller/` + `service/` + `repository/` (or equivalent layered split) | Domain-shaped app → Tier 3 Bounded Context Map is plausible |
| `cli`, `main.rs`, `bin/` shape with no service deployment | Tool / library → skip Tier 4 by default |
| `LICENSE`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md` already at root | Don't re-emit — reference from README instead |
| `.heal/doc_pairs.json` exists | The pairs file's `doc` paths are the hint set for which paired pages are expected — read those into the plan |

Capture the survey in memory; do not write a survey file. The
output of Phase 5 is the only artefact.

### Phase 2 — Survey (existing scaffold tree)

Walk every existing file under `<scaffold_root>/` plus the
emit-set root files (`README.md`, `CONTRIBUTING.md` when those
are emit-set candidates). For each file, classify per **section
heading**:

| Section pattern | Classification |
|---|---|
| Heading text matches a template heading **and** body fits the template's auto-fill shape (table, fenced diagram, single auto-paragraph, bulleted link list) | **auto-managed** — refresh allowed |
| Heading text matches a template heading **but** body has substantive hand-edits (extra paragraphs, custom subsections, code examples beyond the template, > ~30 % divergence from baseline) | **hand-authored** — preserve |
| Heading is **not** in the template (user-added section) | **user-added** — preserve verbatim |

The classification is **fuzzy** by design — the skill is an
agent, not a regex. Read the file with intent: ask "did the
team take ownership of this section?" not "does it match
exactly?". When in doubt, classify as hand-authored. The cost
of leaving a section stale on this run is small (next run can
catch up); the cost of stomping a hand-edit is high.

Record each emitted page's per-section classification map.
Phase 3 consumes it.

The flag overrides:

- `--force`: skip Phase 2's per-section classification — every
  emit-set page is treated as fully auto-managed. Hand-edits in
  emit-set pages are overwritten. Files outside the emit set
  remain untouched (this is non-negotiable).
- `--missing-only`: skip Phase 2 entirely. Existing emit-set
  pages are logged as `skipped (existing)` and Phase 4 emits
  only the missing ones.

### Phase 3 — Reconcile

Combine Phase 1's codebase signal with Phase 2's existing-tree
classification to decide, per page, what action to take. The
decision is **detection-driven, not interactive**: each
candidate page emits only when Phase 1 found enough signal to
fill it with real content. No `AskUserQuestion` for tier
selection. **No emission of skeleton-only pages.** The user
reviews the emitted tree and removes pages they don't want;
pages that don't apply are absent rather than empty.

Per-page action matrix (default mode, no flags):

| Phase 1 says | Phase 2 says | Action |
|---|---|---|
| Page applies (codebase signal present) | File doesn't exist | **emit fresh** |
| Page applies | File exists, all sections auto-managed | **refresh** — regenerate auto-fill in matching sections; preserve user-added sections |
| Page applies | File exists, some sections hand-authored | **partial refresh** — refresh only auto-managed sections; leave hand-authored sections untouched |
| Page applies | File exists, all sections hand-authored | **preserve** — log `preserved (hand-authored)`, don't write |
| Page doesn't apply (signal silent) | File doesn't exist | no-op |
| Page doesn't apply | File exists | **preserve** — the user authored a page the skill would have skipped; never delete |

This matrix is what makes the skill safe to invoke any number
of times: re-runs flow current codebase signal into the
auto-managed parts of the tree, and never overwrite user work.

#### The emit gate

For every candidate page, ask: *can the codebase fill ≥ ~50%
of this page with detected content?*

- **Yes** → emit, auto-fill everything detectable, leave the
  remaining cells as `<not detected>`.
- **No** → **skip**. Do not emit a skeleton; do not emit a
  page that would ship as 80%+ TODO markers or `<not detected>`
  cells. The user authors the page when they have the input.

#### Always-emit pages (Tier 1 — codebase signal is rich)

These six pages always have enough signal: every project has a
manifest, a primary entry point, and exported symbols.

- `README.md` (root) — only if missing. The "Why this exists"
  section is omitted from the auto-emitted README; the user
  adds it (or doesn't). No `TODO(human):` placeholder for it.
- `<scaffold_root>/index.md`
- `<scaffold_root>/architecture/system-context.md`
- `<scaffold_root>/architecture/overview.md`
- `<scaffold_root>/reference/glossary.md`
- `<scaffold_root>/getting-started.md`

#### Conditionally-emit pages (codebase signal must be present)

Each page emits **only** when its detection trigger fires AND
the resulting auto-fill produces meaningful content.

| Page | Emit when |
|---|---|
| `architecture/module-map.md` | Project has > 1 detected workspace / top-level module **and** at least one has a doc comment / module README the skill can quote. |
| `reference/feature-catalog.md` | CLI subcommands, HTTP route handlers, or a README "Features" section can be parsed. Skip when the catalog would be empty. |
| `decisions/index.md` + `decisions/0000-template.md` | Always — the index page is structural (numbering convention + status legend); the template file is meaningful (real headings the user fills when filing an ADR). |
| `contributing.md` | At least two of: detected branch convention, `.github/PULL_REQUEST_TEMPLATE.md`, CODEOWNERS, formatter / linter configs, CI workflow. Skip when the page would be mostly `<not detected>`. |
| `architecture/runtime-views.md` | At least one entry-point cluster (CLI `main`, HTTP routes, queue consumers, top-level public function) detectable. |
| `reference/api.md` | OpenAPI / proto / GraphQL schema present anywhere in the tree. |
| `architecture/data-model.md` | Migration directory or schema definitions (`migrations/`, `db/migrate/`, `prisma/schema.prisma`, `*.sql` DDL, ORM model files) detected. |
| `architecture/deployment.md` | Deployment artefacts detected (`Dockerfile`, `docker-compose.yml`, `k8s/`, `helm/`, `terraform/`, `pulumi/`, `serverless.yml`). |
| `architecture/crosscutting.md` | At least three of the cross-cutting axes have detectable evidence (logging library, error pattern, auth middleware, transaction wrapper, i18n library, cache library). Skip when fewer than three would auto-fill. |
| `strategy/test-strategy.md` | `[features.test]` enabled in `.heal/config.toml`, OR a recognised test framework with > 10 tests detected. |

#### Pages the skill does **not** generate

These page kinds are **not emitted** on first run, because
their content is organisational / forward-looking / incident-
reactive — the codebase has no signal that would let the skill
fill them honestly. They appear in the user's wiki when the
user authors them; the skill stays out of the way.

- `strategy/quality-goals.md` — stakeholder priorities.
- `architecture/bounded-contexts.md` — domain boundaries
  (organisational, not structural; even a detected DDD layered
  split doesn't give context names or relationships).
- `decisions/roadmap.md` — forward planning.
- `decisions/risks.md` — known issues; the user logs items as
  they arise.
- `operations/service-overview.md` / `operations/slos.md` /
  `operations/oncall-onboarding.md` — owner / oncall / SLO
  targets / dashboards are organisational.
- `operations/runbooks/index.md` + sample — runbooks are
  written when an alert is configured. Until then, the page is
  pure scaffolding.
- `operations/postmortems/index.md` + template — postmortems
  are written when an incident occurs. Until then, the page is
  pure scaffolding.
- `strategy/security.md` — threat model is human-curated; the
  page would be 80%+ `<not detected>` cells on first run.

When the user is ready for one of these (e.g. they want to
start filing ADRs, or the team has agreed Quality Goals), they
create the file by hand — the skill's job is to land the
codebase-derived baseline, not to seed empty boxes.

The one exception: the **ADR template**
(`decisions/0000-template.md`) carries `TODO(human):` markers
in its Context / Decision / Consequences body sections. That
template is the *only* place `TODO(human):` appears in this
skill's output. The template is meaningful, not a skeleton —
its purpose is "copy this file when you file ADR-NNNN," and
the markers tell the writer what to author. Other templates
(runbook sample, postmortem template) are not emitted by
default; they're added when the user creates the matching
operational pages.

### Phase 4 — Emit

For each page in the final plan, emit Markdown using
`references/page-templates.md`. The frontmatter is intentionally
minimal — one field — so regeneration doesn't accumulate
state-management drift:

```yaml
---
title: <page title>
---
```

Everything earlier drafts emitted (Diátaxis tag, audience list,
freshness owner, last review, review cycle, related pages /
code / ADRs) was either recoverable from `git log` or
duplicating information already in the body. The recovery
sources stay authoritative: `git log` for ownership and
freshness, body Markdown links for navigation graph,
`.heal/doc_pairs.json` for doc ⇔ src mapping. See
`references/page-templates.md` §2 for the full rationale.

Body shape per template:

- **Auto-fill aggressively.** The skill has read access to the
  whole tree. For Module Map responsibilities, walk each
  module's top-level doc comment / lib entry / package
  description and write a one-line summary. For glossary
  definitions, walk exported types' rustdoc / TSDoc / Python
  docstrings. For container rationales, link to the language
  ecosystem fact ("Rust + clap" rather than "TODO: rationale").
  For Getting Started commands, derive from manifests directly.
- **`TODO(human):` only inside the ADR template.** That's the
  one and only file where the marker appears in this skill's
  output. Other emitted pages either ship as auto-filled
  content or, for cells the skill genuinely cannot fill,
  `<not detected>`. **Never `TODO(human):` outside the ADR
  template.**
- **Empty over invented.** When the skill cannot infer a value
  confidently (an SLO target, an oncall rotation name), the
  page that would have carried that cell is *not emitted at
  all* (per the emit gate). Do not fall back to inventing or
  to dropping a `<not detected>` placeholder for the user to
  fill — the rule is "skip the page, not skeleton it."

Page-emission rules:

- **Atomic per file.** Write each page with a tmp-file + rename
  (the same idiom `Config::save` uses). A SIGINT mid-emit must
  not leave a half-written page.
- **Per-section reconcile, not whole-file overwrite.** When
  Phase 3's action for a page is "refresh" or "partial
  refresh," walk the existing file section by section.
  Regenerate the auto-managed sections in place; copy the
  hand-authored / user-added sections through verbatim.
  Re-assemble with the original section order preserved
  (don't reshuffle sections the user re-ordered).
- **Stable wording.** When refreshing an auto-managed section
  whose underlying signal hasn't changed, emit byte-identical
  output. Random reflow on every run is the diff-noise that
  makes idempotent tools annoying. Read the existing section
  first; if the auto-fill would produce the same content,
  leave the bytes alone.
- **No directory deletion.** Even with `--force`, the skill never
  deletes a directory it didn't create in this run. Removing
  human work is out of scope.
- **No `index.md` magic.** When emitting under a subdirectory
  (e.g. `operations/runbooks/`), also emit the directory's
  `index.md` so the navigation has a parent to attach to.
- **Cross-link via "See also".** Each page's body has a `## See
  also` section listing forward links; emit every back-link
  too (Feature Catalog ↔ ADR Index, Service Overview ↔ Runbook
  Index, etc.). One-way links breed orphans.
- **No DeepWiki sidecar by default.** A `.devin/wiki.json`
  steering file is useful when the user runs DeepWiki, but
  emitting one without prompting is over-reach. Mention as a
  follow-up in the summary.

### Phase 5 — Report

End with a structured summary so the user can `git add -p` with
context:

```
Doc scaffold (reconcile mode):
  scaffold_root:        .heal/docs/
  emitted fresh:        4   (created — file did not exist)
                        .heal/docs/architecture/system-context.md
                        .heal/docs/architecture/overview.md
                        .heal/docs/reference/feature-catalog.md
                        .heal/docs/decisions/0000-template.md
  refreshed (full):     5   (auto-managed file; codebase signal
                             flowed in)
                        README.md
                        .heal/docs/index.md
                        .heal/docs/reference/glossary.md
                        .heal/docs/getting-started.md
                        .heal/docs/architecture/module-map.md
  refreshed (partial):  2   (auto-managed sections refreshed;
                             hand-authored sections preserved)
                        .heal/docs/contributing.md
                        .heal/docs/architecture/deployment.md
  preserved:            1   (entirely hand-authored — left untouched)
                        .heal/docs/decisions/index.md
  unchanged:            3   (auto-managed, no signal change since
                             last run — bytes identical)
                        .heal/docs/reference/api.md
                        .heal/docs/architecture/runtime-views.md
                        .heal/docs/architecture/crosscutting.md
  not emitted (signal silent — left absent):
                        bounded-contexts, data-model,
                        quality-goals, roadmap, risks,
                        test-strategy, security,
                        service-overview, slos, runbooks,
                        postmortems, oncall-onboarding
  TODO(human):          1 (only inside decisions/0000-template.md)
  next steps:
    - When the skeletons read well, promote: `git mv .heal/docs docs`
      and set `[features.docs] scaffold_root = "docs"`.
    - Run /heal-doc-pair-setup to register the new pages.
    - Author the un-emitted pages by hand when the team has the
      input (e.g. file `decisions/0001-xxx.md` from the template,
      author `strategy/quality-goals.md` once the team agrees).
```

## Constraints

- **Autonomous.** No `AskUserQuestion` for tier selection, no
  per-page prompts. Detection signals decide what emits.
- **No skeleton-only pages.** A page emits only when the
  codebase can fill it with meaningful content. Pages that
  would ship as a stack of `TODO(human):` markers or
  `<not detected>` cells are simply not emitted — the user
  authors them when they have the input. The wiki on day 1 is
  smaller than the catalog's full range, on purpose.
- **`TODO(human):` only inside the ADR template.** That single
  file (`decisions/0000-template.md`) is the only place the
  marker ever appears. Other operational templates (runbook
  sample, postmortem template) are skipped on first run. New
  `TODO(human):` markers anywhere else are a violation of the
  autonomy contract.
- **No invented values.** When the skill cannot fill a cell
  honestly, the rule is "skip the page" — not "fall back to
  `<not detected>` and emit anyway." Plausible-sounding made-
  up content (invented owner names, made-up SLO numbers,
  hand-waved security policy) is worse than an absent page;
  see `.claude/docs/doc-scaffold-design.md` §6.3.
- **Idempotent / safe to re-run.** Repeat invocations flow
  current codebase signal into auto-managed sections without
  disturbing hand-edits. Default mode reconciles per-section
  (Phase 2 + Phase 3); `--missing-only` adds new pages only;
  `--force` regenerates emit-set pages from scratch (overrides
  hand-edits — explicit user choice). Files outside the emit
  set are sacred in **all** modes — even `--force` never
  touches them.
- **Stable byte output when signal hasn't changed.** When a
  refresh would produce the same content as what's already on
  disk, leave the file untouched. Random reflow on every run
  is friction nobody asked for.
- **One scaffold root.** Pages all live under
  `[features.docs] scaffold_root`. README and CONTRIBUTING are
  the two repository-root exceptions (their convention is GitHub-
  level, not Wiki-level).
- **Page count ≤ 30.** Tier 1+2+3+4+5 maxima sum to 28; the
  six-category top-level structure (Quick Start / Architecture /
  Reference / Operations / Decisions / Contributing) keeps
  navigation flat. If the user asks for more pages, push back —
  large Wikis are read by no one (`references/wiki-organization.md`
  §3).
- **No deletion of human-authored files.** Even under `--force`,
  the skill writes / overwrites generated pages; it does not
  remove pages the user created themselves outside the page plan.
- **English skeletons by default.** The skill's user-visible
  prose, frontmatter keys, and `TODO(human):` markers are
  English (workflow.md R6.1). When the project's existing docs
  are in a non-English language, mention the locale split in the
  summary so the user can decide whether to translate the
  skeletons or write replacements in the project's language. Do
  not auto-translate.
- **Read-only on source.** This skill writes only Markdown
  skeletons under `<scaffold_root>/` plus (conditionally) a root
  `README.md` / `CONTRIBUTING.md` when those are missing. It
  never edits source code, never edits `.heal/*` (the pair file
  is `/heal-doc-pair-setup`'s job), never edits `config.toml`.
- **No commits.** This skill produces files. Commit boundaries
  are the user's choice — typically one commit per tier, or one
  per phase, but the skill does not auto-commit.
