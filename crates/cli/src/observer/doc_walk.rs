//! Shared discovery logic for Layer B "standalone prose docs"
//! (`README.md`, concept guides, architecture notes). The three Layer B
//! observers (`doc_link_health`, `orphan_pages`, `todo_density`) all
//! need the same answer: "give me every doc file that matches
//! `features.docs.standalone.include` and isn't trimmed by
//! `features.docs.standalone.exclude` or the project-wide ignores."

use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::WalkBuilder;

use crate::core::config::Config;

/// Walk `root` and return every doc file whose project-relative path:
///
/// 1. Is included by at least one entry in
///    `cfg.features.docs.standalone.include`.
/// 2. Is **not** excluded by any entry in
///    `cfg.features.docs.standalone.exclude`.
/// 3. Is **not** excluded by the project's existing
///    `exclude_lines()` (gitignore + workspace-translated).
///
/// The returned paths are project-root-relative (forward-slash form on
/// POSIX) so they line up with what skills and findings already use.
#[must_use]
pub fn walk_standalone_docs(root: &Path, cfg: &Config) -> Vec<PathBuf> {
    let standalone = &cfg.features.docs.standalone;
    let Some(include) = build_matcher(root, &standalone.include) else {
        return Vec::new();
    };
    let exclude = build_matcher(root, &standalone.exclude);
    let project_excludes = build_matcher(root, &cfg.exclude_lines());

    let mut out: Vec<PathBuf> = Vec::new();
    for entry in WalkBuilder::new(root)
        .require_git(false)
        .build()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let abs = entry.into_path();
        let rel = match abs.strip_prefix(root) {
            Ok(p) => p.to_path_buf(),
            Err(_) => continue,
        };
        if !is_match(&include, &rel) {
            continue;
        }
        if let Some(ex) = exclude.as_ref() {
            if is_match(ex, &rel) {
                continue;
            }
        }
        if let Some(ex) = project_excludes.as_ref() {
            if is_match(ex, &rel) {
                continue;
            }
        }
        out.push(rel);
    }
    out.sort();
    out
}

fn build_matcher(root: &Path, lines: &[String]) -> Option<Gitignore> {
    if lines.is_empty() {
        return None;
    }
    let mut builder = GitignoreBuilder::new(root);
    for line in lines {
        // Lines are validated at config load (`Config::validate`); a
        // pattern that fails to compile here is a bug — let the matcher
        // degrade gracefully rather than panic, by silently skipping.
        let _ = builder.add_line(None, line);
    }
    builder.build().ok()
}

fn is_match(gi: &Gitignore, rel: &Path) -> bool {
    // `matched_path_or_any_parents` yields Ignore when any ancestor is
    // matched too, which is what we want for `**/adr/**` style trees.
    gi.matched_path_or_any_parents(rel, /*is_dir=*/ false)
        .is_ignore()
}
