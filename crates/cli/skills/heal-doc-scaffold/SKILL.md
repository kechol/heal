---
name: heal-doc-scaffold
description: Stand up a project's documentation tree from scratch — autonomously, from codebase signals alone (no `AskUserQuestion` calls). Emits **only** pages the codebase can fill meaningfully: container lists, module responsibilities, glossary seeds, getting-started commands, API references, ER tables, runtime sequence diagrams, contributing rules — all derived from manifests, source comments, IaC, and CI configs. Pages whose value comes from organisational decisions (Quality Goals, Bounded Context Map, Roadmap, Service Overview, SLOs, Runbooks, Postmortems, Security Posture) are **not generated as skeletons** — they're skipped on first run and added when the user has the relevant input. `TODO(human):` markers ship in only one file: the ADR template (`decisions/0000-template.md`). Output lands under `[features.docs] scaffold_root` (default `.heal/docs/`). Default mode reconciles an existing tree (refreshes auto-managed sections, preserves hand-edits); `--missing-only` only adds new pages; `--force` regenerates emit-set pages from scratch. Trigger on "scaffold the docs tree", "generate the wiki", "build the documentation from scratch", "/heal-doc-scaffold".
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

A typical mature codebase wants 20–28 pages, not 200. The skill
emits **only** pages whose content the codebase can fill — a
page that would ship as a stack of `TODO(human):` markers or
`<not detected>` cells is not emitted at all. The user authors
those later when they have the relevant input (an incident
produces a postmortem; a roadmap decision drives the Roadmap
page).

The single exception: the ADR template
(`decisions/0000-template.md`) ships with `TODO(human):`
markers in its body — that template is meaningful on its own
(its purpose is "copy this when you file ADR-NNNN"). No other
file in this skill's output ever contains `TODO(human):`.

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

## Output language

Write the conversation, the plan, and the per-page report in the
user's language. Resolution order:

1. Explicit instruction in the current conversation.
2. The language the user is writing in (the chat conversation
   language exposed by the host agent — Claude Code, Codex CLI, …).
3. `[project].response_language` in `.heal/config.toml` (free-form:
   `"Japanese"`, `"日本語"`, `"ja"`, `"français"`).
4. English (fallback).

The **emitted Markdown** itself follows the same resolution: if the
project is set to Japanese, generated headings, body prose, and
`<!-- heal:scaffold:* -->`-bracketed sections are written in
Japanese. Identifiers that are part of the contract stay verbatim —
file paths, frontmatter keys, section markers
(`<!-- heal:scaffold:overview start -->`), Tier names (Tier 1 –
Tier 5), Diátaxis labels (Tutorial / How-to / Reference /
Explanation), arc42 / C4 section names, and command names
(`heal status`). The ADR template (`decisions/0000-template.md`)
keeps its `TODO(human):` markers verbatim — they are a literal
agreement, not prose.

## Pre-flight

1. **Configured root.** Read `[features.docs] scaffold_root` from
   `.heal/config.toml` (default `.heal/docs/`). That default
   keeps scaffold output under `.heal/` to avoid colliding with
   an existing `docs/` tree (Starlight, mdBook, mkdocs); after
   review users typically `git mv .heal/docs docs` and set
   `scaffold_root = "docs"`. Whatever value is set on this run
   is the only tree the skill writes to.
2. **`[features.docs]` awareness.** Probe with
   `heal status --feature docs --json`. If the family is off,
   note in the summary that observers won't see the new pages
   until enabled — but continue. This skill doesn't require the
   family to be on.
3. **Existing tree mode.** Inspect `<scaffold_root>/`:
   - Empty / missing → emit from scratch.
   - Non-empty → reconcile (Phase 2 + Phase 3).
   - `--missing-only` → emit only missing pages; leave existing
     files untouched.
   - `--force` → regenerate emit-set pages from scratch,
     overriding hand-edits in those pages.
   - In **all** modes, files outside the emit set (user-authored
     pages — Quality Goals, Roadmap, filed ADRs, runbooks, etc.)
     are never touched.
4. **Worktree state.** Dirty worktree is OK; mention it in the
   summary so the reviewer knows the diff mixes unrelated work
   with scaffold output.

## Procedure (Detect → Survey → Reconcile → Emit → Report)

First-run and Nth-run share the same pipeline; an existing tree
becomes input that Phase 2 + Phase 3 reason about, which is what
makes the skill safe to re-run.

### Phase 1 — Detect (codebase)

Survey the project so the page plan reflects what's actually
here. Read-only.

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

Classification is **fuzzy** — read the file with intent ("did
the team take ownership of this section?"), not by regex match.
When in doubt, classify as hand-authored: stomping a hand-edit
is more expensive than leaving an auto-managed section stale
for one run.

Flag overrides:

- `--force`: skip per-section classification; treat every
  emit-set page as fully auto-managed.
- `--missing-only`: skip Phase 2 entirely; existing emit-set
  pages are logged `skipped (existing)`.

### Phase 3 — Reconcile

Combine Phase 1's codebase signal with Phase 2's classification
to decide per-page action. Detection-driven, not interactive: a
candidate page emits only when Phase 1 found enough signal to
fill it with real content. Pages that don't apply are absent,
not empty.

Per-page action matrix (default mode, no flags):

| Phase 1 says | Phase 2 says | Action |
|---|---|---|
| Page applies (codebase signal present) | File doesn't exist | **emit fresh** |
| Page applies | File exists, all sections auto-managed | **refresh** — regenerate auto-fill in matching sections; preserve user-added sections |
| Page applies | File exists, some sections hand-authored | **partial refresh** — refresh only auto-managed sections; leave hand-authored sections untouched |
| Page applies | File exists, all sections hand-authored | **preserve** — log `preserved (hand-authored)`, don't write |
| Page doesn't apply (signal silent) | File doesn't exist | no-op |
| Page doesn't apply | File exists | **preserve** — the user authored a page the skill would have skipped; never delete |

#### The emit gate

For every candidate page, ask: *can the codebase fill ≥ ~50% of
this page with detected content?*

- **Yes** → emit. Auto-fill everything detectable; leave
  remaining cells as `<not detected>`.
- **No** → skip. Do not emit a skeleton or a page that would
  ship as 80%+ `<not detected>` cells.

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

These pages are not emitted on first run — their content is
organisational, forward-looking, or incident-reactive, and the
codebase has no signal to fill them honestly. The user authors
them when ready (filing an ADR, agreeing on Quality Goals, etc.).

- `strategy/quality-goals.md` — stakeholder priorities.
- `architecture/bounded-contexts.md` — domain boundaries
  (organisational, not structural).
- `decisions/roadmap.md` — forward planning.
- `decisions/risks.md` — known issues; logged as they arise.
- `operations/service-overview.md` / `operations/slos.md` /
  `operations/oncall-onboarding.md` — owner / oncall / SLO
  targets / dashboards are organisational.
- `operations/runbooks/index.md` + sample — written when an
  alert is configured.
- `operations/postmortems/index.md` + template — written when
  an incident occurs.
- `strategy/security.md` — threat model is human-curated.

The single exception is the ADR template
(`decisions/0000-template.md`), which ships with `TODO(human):`
markers in its Context / Decision / Consequences sections. That
template's purpose is "copy this when you file ADR-NNNN" — the
markers tell the writer what to author. Other templates
(runbook sample, postmortem template) are not emitted until the
user creates the matching operational pages.

### Phase 4 — Emit

For each page in the final plan, emit Markdown using
`references/page-templates.md`. Frontmatter is one field — so
regeneration doesn't accumulate state-management drift:

```yaml
---
title: <page title>
---
```

Ownership and freshness recover from `git log`; navigation graph
recovers from body Markdown links; doc ⇔ src mapping lives in
`.heal/doc_pairs.json`. See `references/page-templates.md` §2.

Body content rules:

- **Auto-fill aggressively.** Walk the tree for content: module
  doc comments → Module Map summaries; exported-type docstrings
  → glossary definitions; manifests → Getting Started commands;
  ecosystem facts ("Rust + clap") → container rationales.
- **`TODO(human):` only inside `decisions/0000-template.md`.**
  Every other emitted page ships as auto-filled content or
  `<not detected>` cells. Markers anywhere else violate the
  autonomy contract.
- **Empty over invented.** If the skill cannot fill a cell
  honestly, the rule is "skip the page" — not "invent" and not
  "drop a `<not detected>` placeholder for the user to fill."

Emission rules:

- **Atomic.** Tmp-file + rename (same idiom as `Config::save`).
  A SIGINT mid-emit must not leave a half-written page.
- **Per-section reconcile.** For "refresh" / "partial refresh"
  actions, walk the file section by section: regenerate
  auto-managed sections in place, copy hand-authored /
  user-added sections through verbatim, preserve the user's
  section order.
- **Stable bytes.** When a refresh would produce identical
  content to what's on disk, leave the file untouched. Random
  reflow on every run is friction nobody asked for.
- **No deletion.** Even under `--force`, never delete a file
  or directory the skill didn't create in this run.
- **`index.md` per subdirectory.** Subdirectories
  (`operations/runbooks/`, etc.) need an `index.md` so the
  nav graph has a parent to attach to.
- **Bidirectional "See also".** Forward links and back-links
  both — one-way links breed orphans.
- **No DeepWiki sidecar.** A `.devin/wiki.json` is over-reach
  without prompting; mention as a follow-up in the summary.

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

- **Autonomous.** No `AskUserQuestion`, no per-page prompts —
  detection signals decide what emits.
- **No skeleton-only pages.** Pages that would ship as 80%+
  `TODO(human):` or `<not detected>` cells are not emitted.
  The user authors them when they have the input.
- **`TODO(human):` only inside `decisions/0000-template.md`.**
  Anywhere else violates the autonomy contract.
- **No invented values.** Plausible-sounding made-up content
  (owner names, SLO numbers, security policy) is worse than an
  absent page. See `.claude/docs/doc-scaffold-design.md` §6.3.
- **Idempotent.** Default mode reconciles per-section without
  disturbing hand-edits. `--missing-only` adds new pages only;
  `--force` regenerates emit-set pages from scratch. Files
  outside the emit set are never touched, in any mode.
- **One scaffold root.** All pages under
  `[features.docs] scaffold_root` except `README.md` /
  `CONTRIBUTING.md`, which are GitHub-level conventions at the
  repo root.
- **Page count ≤ 30.** Tier maxima sum to 28; large wikis read
  by no one (`references/wiki-organization.md` §3). Push back
  if the user asks for more.
- **English skeletons.** User-visible prose, frontmatter keys,
  and markers are English (workflow.md R6.1). When existing docs
  are non-English, surface the locale split in the summary; do
  not auto-translate.
- **Read-only on source.** Writes Markdown under
  `<scaffold_root>/` and (when missing) root `README.md` /
  `CONTRIBUTING.md`. Never edits source, `.heal/*`, or
  `config.toml`.
- **No commits.** This skill produces files; commit boundaries
  are the user's choice.
