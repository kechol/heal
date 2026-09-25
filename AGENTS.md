# AGENTS.md

Guidance for coding agents working on HEAL. Read supporting documents only
when they apply to the task.

## Project

HEAL is a local Rust CLI (`heal`, crate `heal-cli`) that turns code-health
signals into work for coding agents. Code observers are always enabled;
Docs and Test are opt-in. Findings appear through `heal status`, `heal metrics`,
and `heal diff`; `heal doctor` reports setup state. Skills ship as a Claude
Code plugin in `plugins/heal/`.
See [README.md](./README.md) for the user overview.

## Working agreement

- Make routine implementation decisions autonomously. Preserve user changes
  and stay within scope; review and planning requests do not authorize fixes.
- Finish the requested change, affected documentation, and relevant checks.
  Fix failures caused by your changes and rerun affected checks without asking
  for approval at each step. Respect task-specific completion criteria.
- Ask when missing information materially changes the outcome or an action
  needs authorization. Continue independent work while blocked.
- Commit only when requested. Pushes, PRs, external messages, releases, and
  paid services require explicit authorization. Never bypass hooks.
- User instructions take precedence over repository and skill guidance within
  runtime permissions. If guidance blocks work, identify the exact instruction
  and explain the conflict rather than inventing an approval requirement.

## Repository contracts

- Keep tracked content and public communication free of secrets and private
  context. This is public OSS under MIT OR Apache-2.0; cite borrowed algorithms
  and respect the dependency license policy in `deny.toml`.
- Keep HEAL local-only: no telemetry, HTTP clients, update pings, or cloud sync.
  `git2` accesses local repositories.
- Preserve CLI and JSON contracts, schema semantics, stable finding IDs, and
  deterministic observations unless the request includes changing them.
- Metrics indicate change friction; they are not optimization targets. The
  drain target is Critical with `hotspot=true`.
- Keep each work skill's propose → approve → apply loop intact; nothing is
  written before the user approves. Do not add persistent metrics history,
  automatic recalibration, hooks beyond post-commit (git) and the plugin's
  SessionStart version check, or skills inside the CLI.

## Relevant references

Claude rule files are not automatically loaded by Codex. Read and follow the
rules relevant to the change; supporting docs provide implementation context.

| Work | References |
| --- | --- |
| Product scope | [scope rules](./.claude/rules/scope.md), [design philosophy](./.claude/docs/design-philosophy.md) |
| Naming | [terminology rules](./.claude/rules/terminology.md), [glossary](./.claude/docs/glossary.md) |
| Delivery and releases | [workflow rules](./.claude/rules/workflow.md) |
| Rust source and tests | [invariants](./.claude/rules/invariants.md), [conventions](./.claude/docs/conventions.md) |
| Architecture | [architecture](./.claude/docs/architecture.md) |
| Schemas and persistence | [data model](./.claude/docs/data-model.md) |
| Observers and classification | [observer specifications](./.claude/docs/observers.md) |
| Commands and output | [command contracts](./.claude/docs/commands.md) |
| Skills, settings, init, hooks, and mark commands | [integration rules](./.claude/rules/skills-and-hooks.md), [integration design](./.claude/docs/skills-and-hooks.md) |
| Design alternatives | [prior art](./.claude/docs/prior-art.md) |

References may contain stale facts. Check source and tests before changing
behavior to match a conflicting description; raise unresolved contract
conflicts only when they affect the task.

## Verification

Use checks appropriate to the change. Toolchain requirements are in
`Cargo.toml`; CI gates are in [.github/workflows/ci.yml](./.github/workflows/ci.yml).

```sh
cargo build --workspace --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo deny check
```

Run affected single-language tests with `RUSTFLAGS="-D warnings"` for language
feature changes, license checks for dependency changes, and the minimum Rust
version check for MSRV-sensitive changes. Starlight changes use `npm run build`
in `docs/`. Instruction-only edits need content, tracked-link, and diff checks.
Do not repeat passing checks or broaden testing without a concrete reason.

For CLI, classification, or plugin skill changes, also run the dogfooding flow
in the workflow rules. Preserve user configuration, hooks, and installed tools;
use a disposable checkout and installation prefix when necessary. Report any
required check that could not run.

## Documentation and commits

Use glossary terms consistently. Keep source comments, internal guidance,
README, and English docs in English. Japanese user docs mirror the English
pages in natural Japanese. Comments explain why; user docs address new users.

CLI, JSON, schema, and naming changes include affected tests, English/Japanese
docs, plugin skills, internal references, and a `CHANGELOG.md` Unreleased entry
in the same change. Apply schema bumps and breaking notes where required.

Write commit subjects and bodies in English. Follow Conventional Commits and
explain the reason and key decisions in the body. When IDs exist, use one-line
`Task:`, `Decision:`, or `Supersedes:` trailers; do not invent IDs.

Report the outcome concisely in the user's language, including verification
results and material limitations. Distinguish passed checks from unrun checks.
