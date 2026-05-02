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
#[must_use]
pub(crate) fn walk_supported_files(root: &Path, excluded: &[String]) -> Vec<PathBuf> {
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
            Some(path)
        })
        .collect()
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
