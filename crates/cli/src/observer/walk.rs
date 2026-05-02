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
            if is_path_excluded(&path, excluded) {
                return None;
            }
            if !path_inside(&path, root, include_under) {
                return None;
            }
            Some(path)
        })
        .collect()
}

/// True when `path` lies inside `under` (segment-wise) or `under` is
/// `None`. Handles both absolute paths (filesystem walks via
/// `walk_supported_files_under`) and relative paths (`git2` diffs,
/// which emit paths relative to the repo root). When `path` is
/// relative, the comparison is against `under` directly; when
/// absolute, `under` is joined onto `root` first.
#[must_use]
pub(crate) fn path_inside(path: &Path, root: &Path, under: Option<&Path>) -> bool {
    let Some(under) = under else {
        return true;
    };
    let target = if path.is_absolute() {
        if under.is_absolute() {
            under.to_path_buf()
        } else {
            root.join(under)
        }
    } else {
        // Relative path — always compare against the relative form of
        // `under`. An absolute `under` against a relative `path` cannot
        // match, so bail.
        if under.is_absolute() {
            return false;
        }
        under.to_path_buf()
    };
    // strip_prefix is segment-wise: `pkg/web` matches `pkg/web/foo.ts`
    // but not `pkg/webapp/foo.ts`.
    path.strip_prefix(&target).is_ok()
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
    fn path_inside_returns_true_when_under_unset() {
        let root = PathBuf::from("/proj");
        assert!(path_inside(Path::new("/proj/anywhere"), &root, None));
    }

    #[test]
    fn path_inside_segment_wise_match() {
        // `pkg/web` should NOT match `pkg/webapp/foo.ts` even though
        // the byte-prefix matches — strip_prefix is segment-wise.
        let root = PathBuf::from("/proj");
        let under = PathBuf::from("pkg/web");
        assert!(path_inside(
            Path::new("/proj/pkg/web/foo.ts"),
            &root,
            Some(&under),
        ));
        assert!(!path_inside(
            Path::new("/proj/pkg/webapp/foo.ts"),
            &root,
            Some(&under),
        ));
    }

    #[test]
    fn path_inside_handles_absolute_under() {
        let root = PathBuf::from("/proj");
        let under = PathBuf::from("/other/loc");
        assert!(path_inside(
            Path::new("/other/loc/x.ts"),
            &root,
            Some(&under),
        ));
        assert!(!path_inside(Path::new("/proj/x.ts"), &root, Some(&under)));
    }
}
