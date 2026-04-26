//! Project-tree file discovery for tree-sitter-based observers.
//!
//! Uses `ignore::WalkBuilder` (the same crate `tokei` uses internally) so the
//! walk respects `.gitignore`, skips `.git/`, and ignores hidden files by
//! default. The optional `excluded` substring list overlays user-configured
//! excludes (mirroring `LocObserver::scan`'s contract).

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::lang::Language;

/// Walk `root`, returning every file whose extension dispatches to a
/// supported `Language` and whose stringified path doesn't contain any of the
/// `excluded` substrings.
#[must_use]
pub(crate) fn walk_supported_files(root: &Path, excluded: &[String]) -> Vec<PathBuf> {
    WalkBuilder::new(root)
        // Honor .gitignore even outside a git repo — running `heal status`
        // inside a non-git project (or a sub-tree) should still respect the
        // project's intent.
        .require_git(false)
        .build()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|ft| ft.is_file()))
        .filter_map(|entry| {
            let path = entry.into_path();
            Language::from_path(&path)?;
            if !excluded.is_empty() {
                let s = path.to_string_lossy();
                if excluded.iter().any(|pat| s.contains(pat.as_str())) {
                    return None;
                }
            }
            Some(path)
        })
        .collect()
}
