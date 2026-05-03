# CLAUDE.md

Guidance for Claude Code (and other AI coding agents) working in this
repository. Read this first; details live in linked files.

## Project at a glance

HEAL is a Rust CLI (binary: `heal`, single crate `heal-cli`) that
turns code-health signals into work for AI coding agents. It runs an
observer pipeline (LOC, CCN/Cognitive, Churn, Change Coupling,
Duplication, Hotspot, LCOM), classifies findings against per-codebase
calibration, and surfaces them via `heal status` / `heal metrics` /
`heal diff` plus four bundled Claude skills.

For the user-facing overview see [README.md](./README.md).

## This is a public OSS project

The repo is published under the MIT license. **Every commit, PR,
issue comment, and CHANGELOG line is public** the moment it lands on
`origin/main` (or any pushed branch). Treat the repo accordingly.

- **No private context.** No internal URLs, employer-specific paths,
  customer names, sandbox endpoints, personal data, or secrets in
  source / commits / PR descriptions / issue replies. `gitleaks` runs
  in pre-commit but its allowlist is not exhaustive — be intentional.
- **No "leaked context" via comments.** Comments and commit messages
  must not reveal what someone said in a private conversation, what
  is on the user's screen, or anything sourced from
  `KNOWLEDGE.md` / `TODO.md` / `.prompt` (all gitignored, local-only).
- **External-friendly tone.** PR titles, commit subjects, issue
  comments, error messages, and user-facing strings should read well
  to a stranger who lands here from a Google search. Avoid in-jokes,
  internal shorthand, or aggressive language.
- **No telemetry, no network calls.** HEAL is a local tool. The only
  network access is `git2` against the local repo. Don't add
  HTTP clients, version-check pings, or analytics.
- **Attribution.** When borrowing an algorithm or pattern from a
  paper / blog post / other OSS project, cite it in code comments
  and / or `CHANGELOG.md`. Don't paste code from incompatible
  licenses.
- **Dependencies have license consequences.** `cargo deny check`
  enforces the allowlist in `deny.toml`. Adding a non-MIT-compatible
  dep is a license discussion, not a chore commit.

## Where to find things

| You need… | Read… |
|---|---|
| Layered architecture, end-to-end command flows | [.claude/docs/architecture.md](./.claude/docs/architecture.md) |
| `Finding`, `FindingsRecord`, `Severity`, `Config`, `Calibration` schemas | [.claude/docs/data-model.md](./.claude/docs/data-model.md) |
| Per-observer specs (algorithm, calibration, knobs) | [.claude/docs/observers.md](./.claude/docs/observers.md) |
| Per-subcommand internal contract, exit codes, JSON shapes | [.claude/docs/commands.md](./.claude/docs/commands.md) |
| Skill embedding, settings.json sweep, post-commit hook | [.claude/docs/skills-and-hooks.md](./.claude/docs/skills-and-hooks.md) |
| Workspace-wide conventions (lints, tests, docs co-update) | [.claude/docs/conventions.md](./.claude/docs/conventions.md) |
| **Canonical names** (the term contract) | [.claude/docs/glossary.md](./.claude/docs/glossary.md) |

Prescriptive rules are auto-loaded from `.claude/rules/` per the
[Claude memory docs](https://code.claude.com/docs/en/memory#organize-rules-with-claude%2Frules%2F).
Index: [.claude/rules/README.md](./.claude/rules/README.md).

## Toolchain & commands

Rust pinned via [mise](https://mise.jdx.dev) (`mise.toml`). `cargo` is
on `PATH` (via mise activation) or at `~/.cargo/bin/cargo`.

```sh
cargo build  --workspace
cargo test   --workspace
cargo fmt    --all
cargo clippy --workspace --all-targets -- -D warnings
cargo deny   check
```

CI (`.github/workflows/ci.yml`) runs all five — keep them green.

## Dogfooding

After CLI / classification / skill changes:

```sh
cargo install --path crates/cli --force
heal init --force --yes
heal status
```

This repo is HEAL's own test corpus.

## When in doubt

1. Read the relevant `.claude/docs/` page.
2. Check the matching test file (`crates/cli/tests/observer_*.rs`,
   `core_*.rs`).
3. Open an issue before introducing a new dependency, a schema
   change to `.heal/`, or a new persistent file.
