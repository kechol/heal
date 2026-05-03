---
description: Repo-specific commit, PR, and release rules — Conventional Commits with `!`, sweep-in-same-PR, doc co-update, release flow. Generic CI / clippy / fmt is enforced by CI; only repo-specific items are listed here.
---

# Workflow rules

Generic Rust hygiene (`cargo fmt`, `cargo clippy -D warnings`,
`cargo test`, `cargo deny check`) is enforced by CI. The rules below
are the repo-specific items that CI does not catch.

## R1. Conventional Commits with `!` for breaking

```
<type>(<scope>): <imperative summary, ≤72 chars>
<type>!(<scope>): <subject>            # breaking
```

Types in active use: `feat`, `fix`, `refactor`, `docs`, `chore`,
`test`. The `/release` skill reads these to compute the next semver
and generate the CHANGELOG.

The trailing `!` marks any user-visible contract change: CLI flag
rename, JSON field rename, schema version bump, removed command.

## R2. PRs that rename anything sweep in the same PR

Never split a rename into "code PR + docs sweep PR". The follow-up
sweep PR pattern (commits `747f19f`, `e73c537`) is the bug, not the
cure. Sweep targets are listed in `terminology.md` R2.

## R3. Don't commit unless the user asks

Auto-mode does not change this. The default is: make changes, stop,
summarise. The user reviews diffs and runs `/commit` when ready. Do
not chain `commit` onto a task automatically.

## R4. Don't bypass hooks (`--no-verify` etc.)

If a pre-commit hook fails, fix the underlying issue. Don't skip.
If the hook itself is broken, fix the hook in its own PR.

## R5. Doc co-update is part of the change PR

When a CLI flag, output shape, or schema field changes, the same PR
includes:

- Source change.
- Test update.
- Starlight English docs (`docs/src/content/docs/`).
- Starlight Japanese mirror (`docs/src/content/docs/ja/`).
- Affected skill body and references
  (`crates/cli/plugins/heal/skills/<skill>/`).
- `CHANGELOG.md` "Unreleased" entry (`⚠ BREAKING` if applicable).
- `.claude/docs/` and / or `.claude/rules/` if invariants changed.

Forgetting docs == PR not ready.

## R6. Japanese / English split

| Surface | Language |
|---|---|
| `README.md`, `CHANGELOG.md`, `docs/src/content/docs/*` | English |
| `docs/src/content/docs/ja/*` | Japanese (mirror) |
| `.claude/docs/`, `.claude/rules/`, `CLAUDE.md` | English |
| Source comments | English |

When updating Japanese pages, watch for unnatural spaces around CJK
characters — common mechanical-translation artifact.

## R7. Dogfooding loop after CLI / classification / skill changes

```sh
cargo install --path crates/cli --force
heal init --force --yes
heal status
```

This repo is HEAL's test corpus. Self-test surfaces regressions that
unit tests miss (hook script edge cases, skill rendering, plugin
extraction). Pure-internal Rust changes can rely on `cargo test`.

## R8. Release flow

`/release` opens the bump PR. After the PR merges, the maintainer
manually tags the merge commit:

```sh
git switch main && git pull
git tag v0.x.y
git push origin v0.x.y
```

Tagging triggers the release workflow. Don't tag from a feature
branch. Don't force-push tags. Don't `cargo publish` manually.

## R9. `/release`, `/loop`, `/schedule` are user-triggered

The user runs these. Claude does not initiate them — they have
side effects and billing implications. If you finish work that
would benefit, **offer** in a single trailing line.
