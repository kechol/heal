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
| Source comments (`.rs`, `.scm`, `Cargo.toml`, etc.) | English |

When updating Japanese pages, watch for unnatural spaces around CJK
characters — common mechanical-translation artifact.

### R6.1. Internal comments are always English — no exceptions

Every comment that ships in a tracked source file (Rust `//` + `///`
+ `//!`, tree-sitter `.scm` `;`, TOML `#`, shell hook `#`, etc.) is
written in English. The repo is OSS — comments are read by anyone
who clones; mixing languages is friction nobody asked for.

**Don't cite chapter titles from local-only design docs.**
`TODO.md` and `.prompt` are gitignored (see `terminology.md` R3 /
`scope.md` etc.); their chapter titles are Japanese **and** they
drift on every refactor. References like
`(TODO §「Severity と Hotspot は直交した属性」)` end up as stale
broken links in Japanese inside the published source. Inline the
*reasoning* instead — one sentence in English explaining the
**why** is more durable than a §pointer to a moving target.

**Exception: literal example values inside quoted strings.**
`/// values like `"Japanese"`, `"日本語"`, `"ja"`, `"français"`` is
**content** illustrating what a user might pass to a free-form
field, not a comment in Japanese. Keep the example multilingual
when that's what the field accepts.

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
