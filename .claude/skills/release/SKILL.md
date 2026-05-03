---
name: release
description: Cut a release of heal-cli. Reads Conventional Commits since the last tag, picks the next semver, bumps versions in Cargo.toml + Cargo.lock, rewrites the CHANGELOG.md "Unreleased" section into a versioned entry, and opens a `release/vX.Y.Z` PR. Does NOT tag and does NOT publish — those happen after the PR merges. Trigger on "/release", "cut a release", "bump heal version", "prep the next release".
---

# release

Open a release-bump PR for heal-cli. The skill walks the commits
since the last release tag, classifies them, computes the next
semver, and opens a PR that bumps every version reference. Tagging
and publishing happen later — this skill stops at the PR.

## Mental model

`release.yml` triggers on a pushed tag matching
`**[0-9]+.[0-9]+.[0-9]+*`. So the release flow is:

1. **(this skill)** Open `chore(release): bump to vX.Y.Z` on a
   `release/vX.Y.Z` branch.
2. Maintainer reviews + merges.
3. Maintainer tags the merge commit: `git tag vX.Y.Z && git push
   origin vX.Y.Z`. cargo-dist builds the artefacts and the
   crates.io publish step fires.

Steps 2-3 stay manual. The skill never tags, never pushes to `main`,
never runs `cargo publish`.

## Pre-flight

Refuse to start when any of these fail:

1. **Clean worktree.** `git status --porcelain` is empty.
2. **On `main`, in sync with origin.** `git fetch origin` then both
   `git rev-list --count origin/main..HEAD` and
   `git rev-list --count HEAD..origin/main` are 0.
3. **`gh` authenticated.** `gh auth status`.
4. **At least one commit since the last tag.** Otherwise nothing
   to release.
5. **No existing `release/v*` branch on the remote.** `gh api
   repos/{owner}/{repo}/branches | jq '.[].name | select(test("^release/"))'`
   should return nothing. Manual cleanup beats a half-merged bump.

State which check failed and stop on the first failure.

## Picking the next version

1. **Current version** — read `[workspace.package] version` in
   `Cargo.toml` (the source of truth; every crate inherits via
   `version.workspace = true`).
2. **Last tag** — `git describe --tags --match 'v*' --abbrev=0`.
3. **Commits since** — `git log <tag>..HEAD --format='%H%x09%s%x09%b%x00'`
   (NUL-terminate; commit bodies contain newlines).
4. **Classify each commit** (Conventional Commits):
   - body has `BREAKING CHANGE:` OR subject has `!:` after the type
     → **breaking**
   - subject starts with `feat[(scope)]:` → **feature**
   - subject starts with `fix[(scope)]:` → **fix**
   - everything else (`chore`, `ci`, `docs`, `refactor`, `test`,
     `build`, `perf`, `style`) → noted, doesn't drive the bump
5. **Highest-wins bump**:
   - any breaking → **major**
   - else any feature → **minor**
   - else any fix → **patch**
   - else (only chore/docs/etc.) → **patch**, but flag in the summary
     that this is a docs/build-only release; let the user override.
6. **Pre-1.0 convention** (`major == 0`): collapse "breaking" and
   "feature" both to **minor**. Patch stays patch. Keep this
   transparent in the printed plan so the user can override.

Print the plan before any write:

```
Current: v0.1.0
Last tag: v0.1.0  (HEAD: 21d7f0a)
Commits since: 24 total
  feat:        5
  fix:         3
  breaking:    1   (pre-1.0 → counted as minor)
  chore/docs:  15
Proposed: v0.2.0 (minor)
Branch:   release/v0.2.0
```

Ask the user to confirm or override the version. Validate any
override is strictly greater than the current.

## Files to bump

- `Cargo.toml` — `[workspace.package]` `version = "X.Y.Z"`.
- `Cargo.lock` — refresh by running `cargo update -p heal-cli`. If
  that's a no-op, run `cargo build --workspace` to settle the lock.
- `CHANGELOG.md` — see the next section. Mandatory.

Do NOT touch:

- `crates/cli/Cargo.toml` — inherits via `version.workspace = true`.
- `crates/cli/plugins/heal/skills/*/SKILL.md` — the `metadata:` block
  is auto-injected by `skill_assets::extract` on build using
  `env!("CARGO_PKG_VERSION")`. Bumping `Cargo.toml` is enough.
- `docs/package.json` — independent versioning for the docs site.
- `README.md` / `docs/` prose — unless the user asks; version strings
  there are typically advisory.

## Updating `CHANGELOG.md`

`CHANGELOG.md` is part of the release contract — `crates.io`, the
GitHub Release page, and downstream consumers all read it. Bumping
the version without updating the changelog is incomplete.

The flow is **rename + categorise + reset**:

1. Rename the existing `## Unreleased` heading to
   `## vX.Y.Z — YYYY-MM-DD` (today's date, UTC).
2. Inside the renamed section, **categorise** the existing
   `### ⚠ BREAKING`, `### Features`, `### Fixes`, `### Chore`
   sub-sections. Most "Unreleased" content is already organised by
   theme — keep meaningful section headings, but ensure every entry
   has the right severity bucket. Reorder so the section order is:

   ```
   ### ⚠ BREAKING — <one-line headline>
   <details>

   ### Features
   - ...

   ### Fixes
   - ...

   ### Chore
   - ...
   ```

3. **Cross-check git log**: `git log <last-tag>..HEAD --format='%h %s'`.
   For each commit subject (Conventional Commits):
   - `feat(...)` / `feat!(...)` → must appear under Features
     (or BREAKING if `!`).
   - `fix(...)` → Fixes.
   - `refactor!`, `docs!` with user-visible impact → BREAKING.
   - `chore`, `test`, plain `refactor`, plain `docs` → optional;
     include only if they shipped a user-visible improvement
     (dep bump that fixed a CVE, doc rewrite, etc.).
   Any commit missing from the section gets a one-line bullet with
   short SHA: `- <description> (\`<short-sha>\`)`.
4. Re-add a fresh empty `## Unreleased` heading at the top of the
   file (above the new versioned section) so future PRs have a
   landing zone:

   ```markdown
   # Changelog

   ## Unreleased

   ## vX.Y.Z — YYYY-MM-DD

   ### Features
   - ...
   ```

5. Style:
   - Plain English, ≤ 80 char lines, prose ok.
   - Lead each bullet with **what changed for the user**, not the
     internal mechanism.
   - For BREAKING entries, include a migration table or a
     "Migration:" paragraph showing before / after.
   - Reference SHAs sparingly — only when the user might want to
     `git show` for context. Don't link every bullet.

If `CHANGELOG.md` doesn't exist (genuinely fresh repo), create it with
just `# Changelog` then the new versioned section.

## Branch + commit shape

```sh
git switch -c release/vX.Y.Z
# ...edits + cargo update + CHANGELOG rewrite...
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore(release): bump to vX.Y.Z"
```

Commit body: a categorised summary, ~6 lines per category, most
relevant first. Format:

```
chore(release): bump to v0.2.0

Highlights since v0.1.0:

Features
- feat(observer): LCOM approximation (64a848c)
- feat(skill): /heal-code-fix drains the findings cache (60125d5)
- ...

Fixes
- fix(observer): ... (abc1234)

Breaking
- (none — pre-1.0 minor bump)
```

If a `BREAKING CHANGE:` trailer exists, quote it verbatim so review
context is in-band.

## Opening the PR

```sh
git push -u origin release/vX.Y.Z
gh pr create \
  --base main \
  --head release/vX.Y.Z \
  --title "chore(release): bump to vX.Y.Z" \
  --body "$(cat <<'EOF'
## Summary
- Bumps heal-cli from vA.B.C → vX.Y.Z
- CHANGELOG.md "Unreleased" promoted to vX.Y.Z section
- Full changelog below

## Changelog

<paste the new vX.Y.Z section from CHANGELOG.md verbatim>

## Release checklist (after merge)
- [ ] Tag the merge commit: `git tag vX.Y.Z && git push origin vX.Y.Z`
- [ ] Confirm `release.yml` ran (cargo-dist artefacts + crates.io publish)
- [ ] Confirm GitHub Release page is populated
EOF
)"
```

Print the PR URL when done.

## Sanity checks before pushing

Run these locally before `git push`. If anything fails, leave the
branch local and surface the error:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo deny check`

The pre-commit hook already runs `cargo fmt --check` + `gitleaks`,
but the rest are CI-side; running them locally avoids a red CI on
the release PR.

## Constraints

- **Never tag.** Tagging triggers `release.yml`; that's the
  maintainer's call.
- **Never push to `main`.** PRs only.
- **Never `cargo publish`.** Done by `release.yml` after the tag.
- **No `--no-verify`.** Pre-commit hooks must pass.
- **One in-flight release branch at a time** — pre-flight refuses
  if `release/v*` already exists on the remote.

## When NOT to act

- Working tree dirty / branch out of sync / `gh` not authenticated —
  pre-flight catches this; restate the failed check.
- Hotfix that needs to bypass `main` — out of scope; do manually.
- CI on `main` is red — flag it (surface
  `gh run list --branch main --limit 1`) and ask before continuing.
  Don't gate the release on CI, but make sure the user sees the colour.
