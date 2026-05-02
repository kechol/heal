//! Project-tree file discovery for tree-sitter-based observers.
//!
//! Uses `ignore::WalkBuilder` (the same crate `tokei` uses internally) so the
//! walk respects `.gitignore`, skips `.git/`, and ignores hidden files by
//! default. User-configured excludes are evaluated through
//! [`ExcludeMatcher`], which interprets patterns as `.gitignore` syntax —
//! the same DSL every developer already knows from `.gitignore`,
//! `.dockerignore`, ripgrep, etc.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::WalkBuilder;

use crate::observer::lang::Language;

/// Compiled `.gitignore`-style exclusion matcher used by every observer.
/// Patterns understand the full gitignore DSL: glob (`*`, `?`, `**`),
/// directory-only (`foo/`), root anchoring (`/foo`), negation (`!keep`),
/// `#` comments. Empty pattern lists short-circuit to "match nothing"
/// without paying the matcher cost.
#[derive(Debug)]
pub struct ExcludeMatcher {
    inner: Option<Gitignore>,
}

impl ExcludeMatcher {
    /// Empty matcher — `is_excluded` always returns `false`. Used by
    /// observers that haven't (or don't need to) read the user's
    /// config.
    #[must_use]
    pub fn empty() -> Self {
        Self { inner: None }
    }

    /// Compile `patterns` against `root`. Each entry is a single
    /// `.gitignore` line (relative to `root`). Returns the first
    /// `globset` build error if any pattern is malformed so callers can
    /// surface a precise schema error at config-load time.
    pub fn compile(root: &Path, patterns: &[String]) -> Result<Self, ignore::Error> {
        if patterns.is_empty() {
            return Ok(Self::empty());
        }
        let mut builder = GitignoreBuilder::new(root);
        for line in patterns {
            builder.add_line(None, line)?;
        }
        Ok(Self {
            inner: Some(builder.build()?),
        })
    }

    /// True iff `path` matches an exclude rule (and isn't whitelisted
    /// by a later `!pattern`). Walks ancestors so a directory pattern
    /// like `vendor/` correctly excludes `vendor/foo.ts` — the
    /// straight `matched` check only fires on the directory entry
    /// itself, missing files nested inside.
    ///
    /// `path` may be absolute (walk-based observers from
    /// `WalkBuilder`) or relative to the repo root (git2 diff
    /// observers); both work because `Gitignore` accepts either, with
    /// relative paths interpreted against the builder's `root`.
    #[must_use]
    pub fn is_excluded(&self, path: &Path, is_dir: bool) -> bool {
        let Some(gi) = self.inner.as_ref() else {
            return false;
        };
        gi.matched_path_or_any_parents(path, is_dir).is_ignore()
    }
}

/// Walk `root`, returning every file whose extension dispatches to a
/// supported `Language` and which isn't excluded by `matcher`.
///
/// `include_under` (when set) drops any path that doesn't lie under the
/// given sub-path; the check is segment-wise so `pkg/web` does not
/// match `pkg/webapp`. Used by `heal metrics --workspace <path>` to
/// scope walk-based observers (Complexity, Lcom, Duplication).
#[must_use]
pub(crate) fn walk_supported_files_under(
    root: &Path,
    matcher: &ExcludeMatcher,
    include_under: Option<&Path>,
) -> Vec<PathBuf> {
    // Resolve the workspace target once (an absolute join with `root`)
    // so the per-file check is a single `strip_prefix` instead of a
    // repeated allocation. `WalkBuilder` yields absolute paths, so the
    // target must be absolute too.
    let target = resolve_workspace_target(root, include_under, /*paths_absolute=*/ true);
    WalkBuilder::new(root)
        // Honor .gitignore even outside a git repo — running `heal metrics`
        // inside a non-git project (or a sub-tree) should still respect the
        // project's intent.
        .require_git(false)
        .build()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|ft| ft.is_file()))
        .filter_map(|entry| {
            let path = entry.into_path();
            Language::from_path(&path)?;
            // Workspace check first — single strip_prefix on already-
            // resolved targets is cheaper than a gitignore match.
            if !path_under(&path, target.as_deref()) {
                return None;
            }
            if matcher.is_excluded(&path, /*is_dir=*/ false) {
                return None;
            }
            Some(path)
        })
        .collect()
}

/// Pre-resolve `include_under` into an absolute (or relative, matching
/// the caller's path kind) `PathBuf` so the per-file check
/// [`path_under`] is just one `strip_prefix` per call. `paths_absolute`
/// reflects what the caller will pass to `path_under`: walk-based
/// observers yield absolute paths from `WalkBuilder`; git2 diff
/// observers yield repo-root-relative paths.
#[must_use]
pub(crate) fn resolve_workspace_target(
    root: &Path,
    include_under: Option<&Path>,
    paths_absolute: bool,
) -> Option<PathBuf> {
    let under = include_under?;
    if !paths_absolute {
        // Caller supplies relative paths — comparing against an
        // absolute `under` would never match.
        if under.is_absolute() {
            return None;
        }
        return Some(under.to_path_buf());
    }
    if under.is_absolute() {
        Some(under.to_path_buf())
    } else {
        Some(root.join(under))
    }
}

/// True when `path` lies inside the resolved `target` (segment-wise
/// via `Path::strip_prefix`) or `target` is `None`. Pass the result of
/// [`resolve_workspace_target`] for `target`.
#[must_use]
pub(crate) fn path_under(path: &Path, target: Option<&Path>) -> bool {
    target.is_none_or(|t| path.strip_prefix(t).is_ok())
}

/// Unix-second cutoff for git-history observers: "anything older than
/// `since_days` is out of scope". Returns `i64::MIN` if the system clock is
/// before the epoch (effectively "no cutoff") so every commit is admitted.
#[must_use]
pub(crate) fn since_cutoff(since_days: u32) -> i64 {
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return i64::MIN;
    };
    let secs = i64::try_from(now.as_secs()).unwrap_or(i64::MAX);
    secs.saturating_sub(i64::from(since_days).saturating_mul(86_400))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn path_under_returns_true_when_target_unset() {
        assert!(path_under(Path::new("/proj/anywhere"), None));
    }

    #[test]
    fn path_under_segment_wise_match() {
        // `pkg/web` should NOT match `pkg/webapp/foo.ts` even though
        // the byte-prefix matches — strip_prefix is segment-wise.
        let target = PathBuf::from("/proj/pkg/web");
        assert!(path_under(Path::new("/proj/pkg/web/foo.ts"), Some(&target),));
        assert!(!path_under(
            Path::new("/proj/pkg/webapp/foo.ts"),
            Some(&target),
        ));
    }

    #[test]
    fn resolve_workspace_target_joins_relative_under_for_absolute_paths() {
        let root = PathBuf::from("/proj");
        let under = PathBuf::from("pkg/web");
        let target =
            resolve_workspace_target(&root, Some(&under), true).expect("relative resolves");
        assert_eq!(target, PathBuf::from("/proj/pkg/web"));
    }

    #[test]
    fn resolve_workspace_target_keeps_absolute_under_unchanged() {
        let root = PathBuf::from("/proj");
        let under = PathBuf::from("/other/loc");
        let target =
            resolve_workspace_target(&root, Some(&under), true).expect("absolute resolves");
        assert_eq!(target, PathBuf::from("/other/loc"));
    }

    #[test]
    fn resolve_workspace_target_for_relative_paths_drops_absolute_under() {
        let root = PathBuf::from("/proj");
        let under = PathBuf::from("/abs/elsewhere");
        // Caller supplies relative paths — an absolute `under` cannot
        // match anything, so resolution drops to None (the caller
        // will then treat every check as "not in workspace").
        assert!(resolve_workspace_target(&root, Some(&under), false).is_none());
    }
}
