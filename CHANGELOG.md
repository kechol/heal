# Changelog

## Unreleased

### ⚠ BREAKING — `exclude_paths` is now `.gitignore` syntax

`git.exclude_paths`, `metrics.loc.exclude_paths`, and
`[[project.workspaces]].exclude_paths` previously matched as
case-sensitive **substring** patterns. They now parse as
**`.gitignore`** lines with the full DSL: glob (`*`, `**`, `?`,
`[abc]`), directory-only (`foo/`), root anchoring (`/foo`), negation
(`!keep`), and `#` comments.

**Migration:** most existing configs work without changes. Patterns
that relied on bare keyword substring behaviour need a small edit:

| Old (substring) | New (gitignore) | Why |
|---|---|---|
| `target/` | `target/` (unchanged) | Directory pattern works the same |
| `vendor` | `vendor/` *or* `vendor/**` | Bare keyword used to match `weird-vendor-stuff/`; gitignore matches a file/dir literally named `vendor` only |
| `pkg/web/vendor/` | `pkg/web/vendor/` (unchanged) | Anchored directory pattern works the same |
| `.test.ts` | `*.test.ts` (suffix) *or* `**/.test.ts` (exact basename) | Substring matched any path containing the literal `.test.ts` *anywhere* — usually the user's intent is "files whose name ends in `.test.ts`", so `*.test.ts` is the typical replacement; gitignore basename-globs are unanchored by default so no leading `**/` is needed |

`heal status --refresh` after the upgrade reports the new
`severity_counts`; if a previously-excluded subtree starts surfacing
findings, the cause is almost always a bare-keyword pattern that
needs `/` or `*` decoration.

`Config::validate` (run on every config load) now also verifies each
exclude line parses as gitignore syntax. Malformed patterns surface
as `ConfigInvalid` schema errors before any scan starts.

### Workspace `exclude_paths` is wired

`[[project.workspaces]].exclude_paths` was previously declared in the
schema but inert at scan time. It now applies, scoped to the
declaring workspace via gitignore-line translation:

- `vendor/` under `path = "pkg/web"` → matches `pkg/web/**/vendor/`
- `/dist` (anchored to workspace root) → `/pkg/web/dist`
- `!keep.log` → `!pkg/web/**/keep.log`

Other workspaces are unaffected.

### `heal metrics --workspace <PATH>`

New flag scopes every observer to a sub-path. Loc walks only that
subtree; walk-based observers (Complexity / Lcom / Duplication) drop
out-of-workspace files; git-based observers (Churn / ChangeCoupling)
recompute `commits_considered` against the in-workspace universe so
lift / churn totals stay consistent.

### Cross-workspace coupling Advisory bucket

`change_coupling` pairs whose endpoints belong to *different*
declared workspaces are retagged
`change_coupling.cross_workspace` and parked in the Advisory tier by
default. Configurable via
`[metrics.change_coupling] cross_workspace = "surface" | "hide"`.
