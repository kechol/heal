//! Project-tree file discovery for tree-sitter-based observers.
//!
//! Uses `ignore::WalkBuilder` (the same crate `tokei` uses internally) so the
//! walk respects `.gitignore`, skips `.git/`, and ignores hidden files by
//! default. The optional `excluded` substring list overlays user-configured
//! excludes (mirroring `LocObserver::scan`'s contract).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;

use crate::observer::lang::Language;

/// Walk `root`, returning every file whose extension dispatches to a
/// supported `Language` and whose stringified path doesn't contain any of the
/// `excluded` substrings.
///
/// `include_under` (when set) drops any path that doesn't lie under the
/// given sub-path; the check is segment-wise so `pkg/web` does not
/// match `pkg/webapp`. Used by `heal metrics --workspace <path>` to
/// scope walk-based observers (Complexity, Lcom, Duplication).
#[must_use]
pub(crate) fn walk_supported_files_under(
    root: &Path,
    excluded: &[String],
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
            // Workspace check first — it's a single strip_prefix on
            // already-resolved targets, while is_path_excluded
            // allocates a `to_string_lossy` then iterates patterns.
            // Fast filter before slow filter.
            if !path_under(&path, target.as_deref()) {
                return None;
            }
            if is_path_excluded(&path, excluded) {
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

/// Substring-match exclusion check shared by every observer that walks
/// project paths. Empty `patterns` is the fast path.
#[must_use]
pub(crate) fn is_path_excluded(path: &Path, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let s = path.to_string_lossy();
    patterns.iter().any(|pat| s.contains(pat.as_str()))
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
