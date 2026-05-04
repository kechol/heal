# Page templates

Body templates for the pages the scaffold skill emits. Pages
that the skill does **not** emit on first run (per
`references/page-catalog.md` §2 "Not emitted on first run") have
no template here — the user authors them by hand when they have
the input.

## §1 Auto-fill directives

Every "TODO" placeholder in this file is **a directive to the
skill**, not a marker to leave in the output. The skill replaces
the directive with detected content at emit time:

| Directive in template | Skill action |
|---|---|
| `<auto: …>` | Fill from codebase inspection (manifests, source comments, IaC, CI config). Replace verbatim. |
| `TODO(human): …` | Leave verbatim — appears in **one file only**, the ADR template (`decisions/0000-template.md`). Cues the writer when they copy the template to file the next ADR. |

If the skill cannot fill an `<auto: …>` directive after honest
inspection, the right move is to **skip the page entirely**, not
to drop a `<not detected>` placeholder for the user to fill. A
page that would mostly read as `<not detected>` is exactly the
shape the catalog forbids; the user authors it later.

## §2 The frontmatter block

Every emitted page starts with one field. That's it.

```yaml
---
title: <auto: page title>
---
```

`title` is kept because Starlight / mkdocs / mdBook nav and
sidebars key on it; the H1 alone isn't enough for those
generators. Everything else that earlier drafts of this skill
emitted has been dropped, because each was either recoverable
from git or duplicating information the body already carries:

| Dropped field | Recover via |
|---|---|
| `diataxis`, `audience` | The body's tone and content. Categorisation is a reader/writer judgment, not a tag the skill can usefully assert. |
| `freshness_owner` | `git log --format="%aN" -- <path>` (top contributor) |
| `last_review` / `review_cycle` | `git log -1 --format="%cI" -- <path>` + the `doc_freshness` observer's commit-distance signal |
| `related_pages` / `related_code` / `related_adrs` | Markdown links in the body and `.heal/doc_pairs.json` |

Frontmatter is a state-management burden when it carries fields
the emitter has to keep in sync with elsewhere. The minimum
shape avoids that trap entirely.

The `[features.docs]` `todo_density` observer recognises
`TODO(human):` as a marker; the markers inside the ADR template
keep that file from being a one-off ghost in `todo_density`
output (the template is the obvious source of the markers, by
design). Pages this skill does not emit do not have any TODO
markers to surface — they simply don't exist until the user
writes them.

## §3 Tier 1 templates

### `README.md` (root, when missing)

```markdown
# <auto: project name from manifest>

> <auto: subtitle from manifest's `description` field; if absent,
>  use the first sentence of the project's existing top-level
>  comment in `lib.rs` / `main.py` / `index.ts`>

## Quick start

```sh
<auto: install command from detected toolchain>
<auto: minimal "hello world" run from detected entry point>
```

## Documentation

- [Wiki](./.heal/docs/index.md) — project wiki (auto-generated;
  promote to `docs/` with `git mv` once reviewed).
- [Getting Started](./.heal/docs/getting-started.md)
- [Architecture Overview](./.heal/docs/architecture/overview.md)
- [Glossary](./.heal/docs/reference/glossary.md)

## Contributing

See [Contributing](./.heal/docs/contributing.md).

## License

<auto: license name from LICENSE / Cargo.toml / package.json>
```

### `<scaffold_root>/index.md`

```markdown
# <auto: project name> Wiki

> <auto: subtitle, same source as README>

## Quick Start

- [Getting Started](./getting-started.md)
- [System Context](./architecture/system-context.md)

## Architecture

<auto: emit one bullet per architecture page included in this run>

## Reference

<auto: emit one bullet per reference page included in this run>

## Operations

<auto: emit when Tier 4 was emitted; otherwise omit the section>

## Decisions

- [ADR Index](./decisions/index.md)
<auto: emit roadmap / risks bullets when Tier 5 was emitted>

## Contributing

- [Contributing](./contributing.md)
- [Quality Goals](./strategy/quality-goals.md)
<auto: emit test-strategy / security bullets when Tier 5 was emitted>
```

### `<scaffold_root>/architecture/system-context.md`

```markdown
# System Context

> What this system is, inside the world.

## Diagram

```mermaid
flowchart LR
  user["User"] -->|"<auto: detected interaction; e.g.
                   'invokes CLI', 'sends HTTP request'>"|
                   system[["<auto: project name>"]]
<auto: one arrow per detected external system. Sources:
  - dependency manifests (each runtime DB / API client → one
    external box, labelled with the detected verb e.g. 'queries')
  - CI workflows (publish / deploy targets → one external box)
  - Dockerfile FROM lines (base images that imply external
    runtime services don't apply; skip)>
```

## External actors

<auto: bulleted list, one per detected external system, with the
interaction verb. When detection finds none beyond "User",
emit a single bullet for User and skip the section if even that
isn't a sensible fit (pure library project).>

## Boundaries

<auto: one paragraph from the manifest's metadata + detected
file layout. e.g. "This system is a single CLI binary
(crates/cli) plus its embedded skill assets. It owns the source
under crates/. It reads the project tree (filesystem) and
external git history (libgit2) but holds no network state.">
```

### `<scaffold_root>/architecture/overview.md`

```markdown
# Architecture Overview

> Container-level view: what's deployed, how containers talk.

## Diagram

```mermaid
flowchart TB
  <auto: one node per detected workspace / service / deploy unit;
   one edge per detected inter-container link
   (HTTP client config, queue config, shared DB pointer)>
```

## Containers

| Container | Technology | Source |
|---|---|---|
<auto: one row per detected workspace / package / service.
Technology = primary language + dominant framework
(`Rust + clap`, `TypeScript + Next.js`, `Python + FastAPI`,
`Go net/http`). Source = workspace path. **No "rationale"
column** — rationale belongs in ADRs; cross-link instead.>

## Communication

<auto: one short paragraph per detected inter-container channel.
Detect by grep for HTTP clients, gRPC clients, queue libs,
DB drivers in each container's manifest. When only one
container exists, emit a single line: "Single-container
project — no inter-container communication.">

## See also

- [System Context](./system-context.md)
- [Module Map](./module-map.md)
- [ADR Index](../decisions/index.md) — rationale lives here.
```

### `<scaffold_root>/reference/glossary.md`

```markdown
# Glossary

> Single source of truth for project-specific vocabulary.

| Term | Aliases | Definition | Code symbol |
|---|---|---|---|
<auto: one row per exported symbol cluster the skill detects.
Sources for the Definition column (in priority order):
  1. The symbol's rustdoc / TSDoc / Python docstring first
     paragraph.
  2. The module-level doc comment if the symbol is the module's
     primary export.
  3. `<not detected>` if both are silent — never invent.
Only emit rows where a real definition was found. A row that
says `<not detected>` for the definition is noise.>

## Adding a term

When introducing new vocabulary, add it here in the same PR.
The glossary is the contract; if the code and the glossary
disagree, one of them is wrong.

## Out of scope

General industry terms (REST, JSON, SQL) don't belong here —
link to authoritative sources instead.
```

### `<scaffold_root>/getting-started.md`

```markdown
# Getting Started

> A fresh-machine developer is testing-green within an hour.

## Prerequisites

<auto: bulleted list from detected toolchain markers:
  - `rust-toolchain.toml` / `rust-toolchain` → Rust version
  - `.nvmrc` / `package.json#engines.node` → Node version
  - `.python-version` / `pyproject.toml` → Python version
  - `go.mod` → Go version
  - `mise.toml` / `asdf.toml` → multi-tool versions
  - System libs grep'd from README / scripts / Dockerfile>

## Setup

```sh
<auto: install command(s). Detect from manifests:
  - cargo: `cargo build`
  - npm/pnpm/yarn/bun: best matching install command
  - pip / poetry / uv: best matching command
  - go: `go mod download`
  - mise / asdf: `mise install`>
```

## Run the tests

```sh
<auto: test command. Detect from manifests:
  - Rust: `cargo test`
  - JS/TS: `npm test` / detected runner
  - Python: `pytest` / `uv run pytest`
  - Go: `go test ./...`>
```

If the tests pass, you have a working environment.

## Make a small change

<auto: pick a low-risk, file-bounded change idea by inspecting
the codebase. Heuristics in priority order:
  1. A constant in a small file with an existing test → suggest
     "rename it; observe the test renames in CI".
  2. A README example command → suggest "tweak its output text;
     run the example".
  3. The CLI's --help output → suggest "add a one-line section
     and verify with `<bin> --help`".
Emit the chosen change with a 4-step recipe. If no plausible
candidate is found, emit a single line: "No obvious tour
candidate detected — try editing a test assertion in
`<auto: smallest test file>` and re-running the tests.">

## Trouble?

<auto: scan README and CONTRIBUTING for an existing
troubleshooting section; if found, link to it. Otherwise emit:
"Open an issue if your fresh-machine setup hits something this
guide missed.">

## See also

- [Contributing](./contributing.md)
- [Architecture Overview](./architecture/overview.md)
```

## §4 Tier 2 templates

### `<scaffold_root>/architecture/module-map.md`

```markdown
# Module Map

> Static structure: what code lives where.

| Module | Responsibility | Depends on | Source |
|---|---|---|---|
<auto: one row per detected workspace / package / top-level
src directory. Responsibility column source:
  1. Module-level doc comment (`//!` in Rust, top-of-file
     docstring in Python, leading TSDoc in TS).
  2. README in the module's directory (first paragraph).
  3. The `description` field in the module's local manifest
     (Cargo.toml `[package]`, package.json).
  4. `<not detected>` — never invent a responsibility.
Depends on column = direct dependencies from the local manifest,
filtered to in-tree packages only.>

## Read this first

The Module Map describes **responsibilities**, not file
hierarchies. Two modules with the same responsibility are a
duplication; two responsibilities in one module are a coupling.

## See also

- [Architecture Overview](./overview.md)
- [Glossary](../reference/glossary.md)
```

### `<scaffold_root>/reference/feature-catalog.md`

```markdown
# Feature Catalog

> Every user-visible feature.

| Feature | Summary | Status | Code entry | Related ADRs |
|---|---|---|---|---|
<auto: one row per detected feature. Sources, in priority order:
  1. The CLI's top-level subcommand list (parse `--help`
     output for binaries).
  2. HTTP route handlers (parse routes from frameworks the
     skill recognises: Express, FastAPI, axum, Gin, Rails).
  3. Public exports tagged with a "feature" doc-comment
     attribute (project-specific; usually skipped).
  4. README "Features" section bullets.
Status defaults to `Stable`. Code entry = file:line of the
detected entry point. Related ADRs = empty array (filled later
when ADRs land).>

If the table emits empty, the project has no detected
externally-visible features yet — that is a valid state for
internal libraries and tools-of-tools.

## See also

- [Architecture Overview](../architecture/overview.md)
- [ADR Index](../decisions/index.md)
- [Roadmap](../decisions/roadmap.md)
```

### `<scaffold_root>/decisions/index.md`

```markdown
# Architecture Decision Records

> Frozen records of important choices. ADRs aren't edited after
> they're accepted — when a decision is overturned, write a new
> ADR that supersedes the old one.

## Convention

- File name: `<NNNN>-<short-slug>.md` (zero-padded 4-digit
  number).
- Status legend: `Proposed` → `Accepted` → `Superseded by NNNN`
  or `Deprecated`.
- Use the [template](./0000-template.md) when starting a new
  ADR.

## Index

| # | Title | Status | Date |
|---|---|---|---|
<empty until ADRs are added — individual ADR bodies are §1
carve-out and are never auto-generated>

## See also

- [Quality Goals & Constraints](../strategy/quality-goals.md)
```

### `<scaffold_root>/decisions/0000-template.md`

```markdown
---
title: ADR Template
---

# ADR-NNNN: <decision title>

## Status

Proposed | Accepted | Deprecated | Superseded by ADR-NNNN

## Context

TODO(human): one paragraph. The situation forcing a decision,
the constraints, the alternatives. **§1 carve-out — judgment
at decision time, can't be reconstructed.**

## Decision

TODO(human): one paragraph. What was decided.

## Consequences

TODO(human): two short lists.

**Good:**

- ...

**Bad:**

- ...
```

### `<scaffold_root>/contributing.md`

```markdown
# Contributing

## Branching

<auto: detect the branching strategy by inspecting the default
branch name and recent merge graph. Emit one of:
  - "Trunk-based — merge to `main` via PR; no long-lived
    branches."
  - "Git-flow — feature branches off `develop`, releases off
    `main`."
  - "<not detected>" if signals conflict.>

## Pull requests

<auto: emit checklist from `.github/PULL_REQUEST_TEMPLATE.md`
when present. Otherwise emit:
"Open a PR against `<auto: default branch>`. Include a clear
description, linked issue (if any), and `Refs:` trailer for
HEAL findings being addressed.">

## Reviews

<auto: emit `.github/CODEOWNERS` summary when present.
Otherwise: "At least one approving review required before
merge.">

## Coding standards

<auto: enumerate detected formatters / linters with their
config files:
  - Rust: rustfmt.toml + clippy in CI
  - JS/TS: prettier + eslint + tsconfig
  - Python: ruff / black / mypy
  - Go: gofmt + golangci-lint
Each on one line.>

## Tests

See [Test Strategy](./strategy/test-strategy.md). Run locally:

```sh
<auto: test command, same source as Getting Started>
```

## Releases

<auto: detect release pattern:
  - Conventional Commits + auto-bump in CI → "Conventional
    Commits drive semver via CI; `<changelog tool>` regenerates
    `CHANGELOG.md` on release."
  - Manual tag → "Maintainer tags `vX.Y.Z` on `main`; release
    workflow publishes."
  - `<not detected>` otherwise.>

## See also

- [Getting Started](./getting-started.md)
- [Quality Goals](./strategy/quality-goals.md)
```

## §5 Tier 3 templates (conditional — emit when detection trigger fires)

### `<scaffold_root>/reference/api.md`

```markdown
# API Reference

> External API surface. The generated reference (OpenAPI / proto
> / GraphQL output) is the authoritative listing; this page is
> the **explanation** beside it.

## Generated reference

<auto: detect the schema file and link to it:
  - `openapi.yaml` / `openapi.json` → "Schema:
    [<path>](<path>) — render with Swagger UI / Redoc."
  - `*.proto` → "Schema:
    [<dir>](<dir>) — render with `protoc --doc_out`."
  - `schema.graphql` → "Schema: [<path>](<path>) — render with
    GraphiQL / Mercurius docs."
Emit a one-line CI hookup hint matching the detected ecosystem.>

## Authentication

<auto: detect from the schema's `securitySchemes` / detected
middleware (JWT / OAuth / API key). Emit a short paragraph
describing the scheme. `<not detected>` when ambiguous.>

## Rate limits

<auto: scan for rate-limit middleware config. `<not detected>`
when nothing is found — never invent a number.>

## Error model

<auto: read the schema's error response shape and summarise.>

## See also

- [Architecture Overview](../architecture/overview.md)
- [Data Model](../architecture/data-model.md)
```

### `<scaffold_root>/architecture/runtime-views.md`

```markdown
# Runtime Views

> Sequence diagrams for **important** scenarios. The skill
> emits one diagram per detected entry-point cluster (5–10
> total target).

<auto: for each detected entry point, emit one section with a
Mermaid sequenceDiagram showing the call path:
  - CLI: `main` → arg parse → command dispatch → output
  - HTTP: route handler → service layer → repository / DB
  - Worker: queue consume → job handler → side-effect
  - Library: top-level public function → internal helpers →
    return
Each section's header is the entry-point name; the prose under
the diagram is one paragraph derived from the function's doc
comment / surrounding code structure. When the auto-derivation
finds nothing meaningful for a sequence, omit that section
rather than emitting an empty `TODO(human):` placeholder.>

## See also

- [Architecture Overview](./overview.md)
- [Module Map](./module-map.md)
```

### `<scaffold_root>/architecture/data-model.md`

```markdown
# Data Model

> Top-level entities, relationships, invariants.

## ER diagram

```mermaid
erDiagram
  <auto: parse migration files / schema.prisma / *.sql DDL /
   ORM models, emit one entity per table / model and one
   relationship line per foreign key>
```

## Entities

<auto: one section per detected entity. Each section's content:
  - Columns table from the schema definition
  - First-paragraph description from any model docstring or
    migration comment, or `<not detected>` for the description
  - List of explicit `CHECK` / `UNIQUE` / `NOT NULL` invariants
    pulled from the DDL
Don't invent invariants the schema doesn't enforce — those
belong in the team's heads, not in this auto-generated page.>

## Migrations

<auto: link to the migration directory.>

## See also

- [API Reference](../reference/api.md)
- [Bounded Contexts](./bounded-contexts.md)
```

### `<scaffold_root>/architecture/deployment.md`

```markdown
# Deployment View

> Where containers run. The IaC files are the source of truth;
> this page summarises them.

## Environments

| Environment | Purpose | URL |
|---|---|---|
<auto: parse Terraform workspaces / Pulumi stacks / Helm value
files / docker-compose service files for environment names.
Purpose = derived from the env name (`prod` → "Production",
`stg` → "Staging"). URL = `<not detected>` unless explicit in
config.>

## Topology

```mermaid
flowchart TB
  <auto: parse Dockerfile + docker-compose / Helm chart /
   k8s manifests for service nodes and their declared
   dependencies. One node per service, one edge per declared
   `depends_on` / service-discovery reference.>
```

## Secrets management

<auto: detect by grep for `Secret` / `SealedSecret` / SOPS
config / direnv / Vault references. Emit one paragraph naming
the detected approach. `<not detected>` when silent.>

## See also

- [Service Overview](../operations/service-overview.md)
```

### `<scaffold_root>/architecture/crosscutting.md`

```markdown
# Crosscutting Concepts

> Decisions that span ≥ 3 modules.

## Error handling

<auto: detect the error type pattern by sampling top-level
function signatures:
  - Rust: `Result<T, E>` with the most common `E` type
  - TS: `Result<T>` / `Either<E, T>` / try-catch
  - Python: exception base class
  - Go: `error` returns + sentinel errors
Emit one paragraph naming the pattern. `<not detected>` when
mixed.>

## Logging

<auto: detect logging library from manifest (`tracing`, `slog`,
`winston`, `pino`, `loguru`, `structlog`, etc.). Emit one
paragraph: library name, structured fields convention if
inferable, log levels in use.>

## Authentication / Authorization

<auto: scan for auth middleware / library imports. Emit a one-
paragraph summary. `<not detected>` for non-application code.>

## i18n

<auto: detect i18n library (`i18next`, `gettext`, `fluent`).
Emit one paragraph. When nothing detected, emit:
"Single-locale project — no i18n machinery in use.">

## Caching

<auto: detect cache library / Redis client / `cache:` HTTP
header conventions. Emit one paragraph. `<not detected>` when
silent.>

## Transaction boundaries

<auto: detect transaction wrapper pattern (`#[transaction]`
attributes, ORM `transaction()` calls, manual `BEGIN/COMMIT`).
Emit one paragraph. `<not detected>` when stateless.>

## See also

- [Architecture Overview](./overview.md)
- [ADR Index](../decisions/index.md)
```

## §6 Tier 5 templates (conditional — emit only when triggered)

### `<scaffold_root>/strategy/test-strategy.md`

```markdown
# Test Strategy

> Layered policy for what gets tested where.

## Pyramid shape

<auto: detect by counting tests per layer:
  - Unit (file count under `tests/` and `*_test.*` naming)
  - Integration (count under `tests/integration/` or similar)
  - E2E (count under `e2e/` / `cypress/` / `playwright/`)
Pick the matching shape: pyramid (more unit), trophy
(integration-heavy), honeycomb (integration core), or report
the raw counts when ambiguous.>

## Per-layer policy

### Unit

<auto: count + the test framework detected from manifests
(`#[test]`, `pytest`, `vitest`, `jest`, etc.).>

### Integration

<auto: count + framework. `<none detected>` when absent.>

### End-to-end

<auto: count + framework. `<none detected>` when absent.>

### Property-based

<auto: detect `proptest`, `quickcheck`, `hypothesis`,
`fast-check`. Emit short summary. `<none detected>` when
absent.>

## Coverage / mutation targets

<auto: detect `lcov.info` paths from `[features.test.coverage]`
config; mention if a coverage tool is wired in CI. Targets
themselves are organisational and stay blank.>

## Flaky tests

<auto: emit a one-line policy stub: "Quarantine flaky tests
with `#[ignore]` / `.skip` and an issue link; do not weaken
the assertion." (User edits if their convention differs.)>

## See also

- [Contributing](../contributing.md)
- [Quality Goals](./quality-goals.md)
```

## §7 Cross-links via "See also"

Cross-links live in the body's `## See also` section, not in
frontmatter. The skill emits these pairs symmetrically (forward
link in A's See also; reciprocal link in B's See also) — and
only between **emitted** pages (a See-also link to a page that
wasn't generated would dead-end the reader):

- `Feature Catalog` ↔ `ADR Index` (when both emit)
- `Architecture Overview` ↔ `Module Map`
- `Architecture Overview` ↔ `System Context`
- `Architecture Overview` ↔ `Deployment View` (when emit)
- `Data Model` ↔ `API Reference` (when both emit)
- `Test Strategy` ↔ `Contributing` (when both emit)

Skip back-links from conventional entry points (`README.md`,
`<scaffold_root>/index.md`) — those are reachable from outside
the doc graph. The `[features.docs]` `orphan_pages` observer
treats See-also Markdown links the same way it treats body
links, so symmetry there is what guards against orphans.
