//! Project-tree file discovery for tree-sitter-based observers.
//!
//! Uses `ignore::WalkBuilder` (the same crate `tokei` uses internally) so the
//! walk respects `.gitignore`, skips `.git/`, and ignores hidden files by
//! default. User-configured excludes are evaluated through
//! [`ExcludeMatcher`], which interprets patterns as `.gitignore` syntax —
//! the same DSL every developer already knows from `.gitignore`,
//! `.dockerignore`, ripgrep, etc.

use std::path::Component;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::WalkBuilder;

use crate::observer::shared::lang::Language;

/// Compiled `.gitignore`-style exclusion matcher used by every observer.
/// Patterns understand the full gitignore DSL: glob (`*`, `?`, `**`),
/// directory-only (`foo/`), root anchoring (`/foo`), negation (`!keep`),
/// `#` comments. Empty pattern lists short-circuit to "match nothing"
/// without paying the matcher cost.
#[derive(Debug, Clone)]
pub struct ExcludeMatcher {
    inner: Option<Gitignore>,
    can_prune: bool,
}

impl ExcludeMatcher {
    /// Empty matcher — `is_excluded` always returns `false`. Used by
    /// observers that haven't (or don't need to) read the user's
    /// config.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            inner: None,
            can_prune: true,
        }
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
            can_prune: !patterns.iter().any(|line| line.starts_with('!')),
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

    fn can_prune_dir(&self, path: &Path) -> bool {
        self.can_prune && self.is_excluded(path, true)
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
    walk_supported_files_under_impl(root, matcher, include_under).0
}

/// True when an existing workspace resolves inside the project root.
/// Canonical paths reject both lexical `..` escapes and symlinks that
/// point outside the project.
#[must_use]
pub(crate) fn workspace_is_within(root: &Path, under: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let target = if under.is_absolute() {
        under.to_path_buf()
    } else {
        root.join(under)
    };
    if let Ok(target) = target.canonicalize() {
        return target.strip_prefix(root).is_ok();
    }

    let Ok(relative) = target.strip_prefix(&root) else {
        return false;
    };
    let mut depth = 0usize;
    for component in relative.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::ParentDir if depth > 0 => depth -= 1,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
            Component::CurDir => {}
        }
    }

    target
        .ancestors()
        .find(|path| path.exists())
        .is_some_and(|path| {
            path.canonicalize()
                .is_ok_and(|path| path.strip_prefix(root).is_ok())
        })
}

fn walk_supported_files_under_impl(
    root: &Path,
    matcher: &ExcludeMatcher,
    include_under: Option<&Path>,
) -> (Vec<PathBuf>, usize) {
    // Resolve the workspace target once (an absolute join with `root`)
    // so the per-file check is a single `strip_prefix` instead of a
    // repeated allocation. `WalkBuilder` yields absolute paths, so the
    // target must be absolute too.
    let target = resolve_workspace_target(root, include_under, /*paths_absolute=*/ true);
    let requested_root = target.as_deref().unwrap_or(root);
    if include_under.is_some_and(|under| {
        under
            .components()
            .any(|component| component == Component::ParentDir)
    }) || !requested_root.exists()
    {
        return (Vec::new(), 0);
    }
    let walk_root = target.as_deref().map_or(root, |target| {
        if workspace_requires_project_root_walk(root, target) {
            root
        } else {
            target
        }
    });
    let prune_matcher = matcher.clone();
    let mut builder = WalkBuilder::new(walk_root);
    builder
        // Honor .gitignore even outside a git repo — running `heal metrics`
        // inside a non-git project (or a sub-tree) should still respect the
        // project's intent.
        .require_git(false)
        .parents(true)
        .filter_entry(move |entry| {
            !entry.file_type().is_some_and(|ft| ft.is_dir())
                || !prune_matcher.can_prune_dir(entry.path())
        });
    let mut files = Vec::new();
    let mut visited = 0;
    for entry in builder.build().filter_map(Result::ok) {
        visited += 1;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.into_path();
        if Language::from_path(&path).is_none()
            || !path_under(&path, target.as_deref())
            || matcher.is_excluded(&path, false)
        {
            continue;
        }
        files.push(path);
    }
    (files, visited)
}

fn workspace_crosses_hidden_path(root: &Path, target: &Path) -> bool {
    target.strip_prefix(root).is_ok_and(|relative| {
        relative.components().any(|component| {
            matches!(component, Component::Normal(name) if name.to_string_lossy().starts_with('.'))
        })
    })
}

fn workspace_requires_project_root_walk(root: &Path, target: &Path) -> bool {
    if workspace_crosses_hidden_path(root, target) {
        return true;
    }
    let Ok(relative) = target.strip_prefix(root) else {
        return true;
    };
    let mut path = root.to_path_buf();
    for component in relative.components() {
        path.push(component);
        if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return true;
        }
    }

    let mut ancestors = Vec::new();
    let mut ancestor = target.parent();
    while let Some(path) = ancestor {
        ancestors.push(path);
        ancestor = path.parent();
    }
    if ancestors.into_iter().rev().any(|dir| {
        [dir.join(".ignore"), dir.join(".gitignore")]
            .into_iter()
            .any(|ignore_file| ignore_file_matches_dir(dir, &ignore_file, target))
            || ignore_file_matches_dir(dir, &dir.join(".git/info/exclude"), target)
    }) {
        return true;
    }

    let (global, error) = Gitignore::global();
    error.is_some() || path_or_ancestor_is_ignored(&global, target, root)
}

fn path_or_ancestor_is_ignored(matcher: &Gitignore, target: &Path, root: &Path) -> bool {
    target
        .ancestors()
        .take_while(|path| *path != root)
        .any(|path| matcher.matched(path, true).is_ignore())
}

fn ignore_file_matches_dir(root: &Path, ignore_file: &Path, target: &Path) -> bool {
    if !ignore_file.is_file() {
        return false;
    }
    let mut builder = GitignoreBuilder::new(root);
    if builder.add(ignore_file).is_some() {
        return true;
    }
    builder.build().map_or(true, |matcher| {
        path_or_ancestor_is_ignored(&matcher, target, root)
    })
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
/// `since_days` before the observed ref is out of scope". Anchoring the
/// window to the ref's commit time keeps a scan stable across execution days.
#[must_use]
pub(crate) fn since_cutoff(reference_secs: i64, since_days: u32) -> i64 {
    reference_secs.saturating_sub(i64::from(since_days).saturating_mul(86_400))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn since_cutoff_is_anchored_to_observed_ref() {
        let commit_time = 2_000_000_000;
        assert_eq!(since_cutoff(commit_time, 90), 1_992_224_000);
        assert_eq!(since_cutoff(commit_time, 0), commit_time);
    }

    #[test]
    fn since_cutoff_saturates_for_extreme_windows() {
        assert_eq!(since_cutoff(i64::MIN + 10, u32::MAX), i64::MIN);
    }

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

    #[test]
    fn workspace_walk_starts_at_target_and_preserves_parent_gitignore() {
        let dir = tempfile::TempDir::new().unwrap();
        let (source_name, source) = crate::observer::shared::lang::test_source_fixture();
        std::fs::create_dir_all(dir.path().join("pkg/web/src")).unwrap();
        std::fs::create_dir_all(dir.path().join("pkg/other/src")).unwrap();
        let ignored_name = format!(
            "ignored.{}",
            Path::new(source_name)
                .extension()
                .unwrap()
                .to_str()
                .unwrap()
        );
        std::fs::write(dir.path().join(".gitignore"), format!("{ignored_name}\n")).unwrap();
        std::fs::write(dir.path().join("pkg/web/src").join(source_name), source).unwrap();
        std::fs::write(dir.path().join("pkg/web/src").join(&ignored_name), source).unwrap();
        for index in 0..100 {
            std::fs::write(
                dir.path().join("pkg/other/src").join(format!(
                    "{index}.{extension}",
                    extension = Path::new(source_name)
                        .extension()
                        .unwrap()
                        .to_str()
                        .unwrap()
                )),
                source,
            )
            .unwrap();
        }
        let matcher = ExcludeMatcher::empty();
        let (files, visited) =
            walk_supported_files_under_impl(dir.path(), &matcher, Some(Path::new("pkg/web")));
        assert_eq!(
            files,
            vec![dir.path().join("pkg/web/src").join(source_name)],
            "parent .gitignore semantics must survive the narrower walk root",
        );
        assert!(
            visited < 10,
            "visited {visited} entries outside the workspace"
        );
    }

    #[test]
    fn workspace_walk_preserves_gitignore_above_project_root() {
        let outer = tempfile::TempDir::new().unwrap();
        let project = outer.path().join("project");
        let (source_name, source) = crate::observer::shared::lang::test_source_fixture();
        std::fs::create_dir_all(project.join("pkg")).unwrap();
        std::fs::write(project.join("pkg").join(source_name), source).unwrap();
        std::fs::write(outer.path().join(".gitignore"), "project/pkg/\n").unwrap();

        assert!(walk_supported_files_under(
            &project,
            &ExcludeMatcher::empty(),
            Some(Path::new("pkg")),
        )
        .is_empty());
    }

    #[test]
    fn custom_excludes_prune_only_without_negation() {
        let dir = tempfile::TempDir::new().unwrap();
        let (source_name, source) = crate::observer::shared::lang::test_source_fixture();
        let extension = Path::new(source_name)
            .extension()
            .unwrap()
            .to_str()
            .unwrap();
        std::fs::create_dir_all(dir.path().join("vendor/keep")).unwrap();
        for index in 0..100 {
            std::fs::write(
                dir.path().join(format!("vendor/{index}.{extension}")),
                source,
            )
            .unwrap();
        }
        let live_name = format!("live.{extension}");
        std::fs::write(dir.path().join("vendor/keep").join(&live_name), source).unwrap();

        let excluded = ExcludeMatcher::compile(dir.path(), &["vendor/".into()]).unwrap();
        let (files, visited) = walk_supported_files_under_impl(dir.path(), &excluded, None);
        assert!(files.is_empty());
        assert!(
            visited < 5,
            "excluded subtree was walked ({visited} entries)"
        );

        let negated = ExcludeMatcher::compile(
            dir.path(),
            &[
                "vendor/".into(),
                "!vendor/keep/".into(),
                format!("!vendor/keep/{live_name}"),
            ],
        )
        .unwrap();
        let (files, negated_visits) = walk_supported_files_under_impl(dir.path(), &negated, None);
        assert_eq!(files, vec![dir.path().join("vendor/keep").join(live_name)]);
        assert!(negated_visits > visited);
    }

    #[test]
    fn workspace_boundary_rejects_absolute_parent_and_symlink_escapes() {
        let project = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(project.path().join("inside")).unwrap();
        assert!(workspace_is_within(project.path(), Path::new("inside")));
        assert!(!workspace_is_within(project.path(), outside.path()));
        assert!(!workspace_is_within(
            project.path(),
            Path::new("../outside")
        ));
        assert!(workspace_is_within(
            project.path(),
            Path::new("removed/workspace")
        ));
        std::fs::create_dir_all(project.path().join("pkg/src")).unwrap();
        assert!(workspace_is_within(
            project.path(),
            Path::new("pkg/../pkg/src")
        ));

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), project.path().join("escape")).unwrap();
            assert!(!workspace_is_within(project.path(), Path::new("escape")));
        }
    }

    #[test]
    fn workspace_walk_preserves_hidden_ancestor_filtering() {
        let dir = tempfile::TempDir::new().unwrap();
        let (source_name, source) = crate::observer::shared::lang::test_source_fixture();
        for workspace in [".hidden/src", "visible/.nested/src"] {
            std::fs::create_dir_all(dir.path().join(workspace)).unwrap();
            std::fs::write(dir.path().join(workspace).join(source_name), source).unwrap();
            let (files, _) = walk_supported_files_under_impl(
                dir.path(),
                &ExcludeMatcher::empty(),
                Some(Path::new(workspace)),
            );
            assert!(files.is_empty());
        }
    }

    #[test]
    fn workspace_walk_honors_hidden_directory_whitelist() {
        let dir = tempfile::TempDir::new().unwrap();
        let (source_name, source) = crate::observer::shared::lang::test_source_fixture();
        std::fs::create_dir_all(dir.path().join(".hidden")).unwrap();
        std::fs::write(dir.path().join(".hidden").join(source_name), source).unwrap();
        std::fs::write(dir.path().join(".gitignore"), "!.hidden/\n").unwrap();

        assert_eq!(
            walk_supported_files_under(
                dir.path(),
                &ExcludeMatcher::empty(),
                Some(Path::new(".hidden")),
            ),
            vec![dir.path().join(".hidden").join(source_name)],
        );
    }

    #[test]
    fn workspace_whitelist_cannot_reinclude_an_excluded_parent() {
        for ignore_name in [".gitignore", ".ignore"] {
            let dir = tempfile::TempDir::new().unwrap();
            let (source_name, source) = crate::observer::shared::lang::test_source_fixture();
            let workspace = Path::new("pkg/nested");
            let source_path = dir.path().join(workspace).join(source_name);
            std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
            std::fs::write(&source_path, source).unwrap();
            let ignore_path = dir.path().join(ignore_name);
            std::fs::write(&ignore_path, "pkg/\n!pkg/nested/\n").unwrap();

            assert!(
                walk_supported_files_under(dir.path(), &ExcludeMatcher::empty(), Some(workspace),)
                    .is_empty(),
                "{ignore_name}: an ignored parent must still block traversal"
            );

            std::fs::write(&ignore_path, "pkg/\n!pkg/\n!pkg/nested/\n").unwrap();
            assert_eq!(
                walk_supported_files_under(dir.path(), &ExcludeMatcher::empty(), Some(workspace),),
                vec![source_path],
                "{ignore_name}: explicitly reincluding the parent permits traversal",
            );
        }
    }

    #[test]
    fn global_ignore_match_checks_workspace_ancestors() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut builder = GitignoreBuilder::new(dir.path());
        builder.add_line(None, "ignored/").unwrap();
        let matcher = builder.build().unwrap();
        assert!(path_or_ancestor_is_ignored(
            &matcher,
            &dir.path().join("ignored/nested"),
            dir.path(),
        ));
    }

    #[test]
    fn workspace_walk_preserves_symlink_and_ignored_root_filtering() {
        let dir = tempfile::TempDir::new().unwrap();
        let (source_name, source) = crate::observer::shared::lang::test_source_fixture();
        std::fs::create_dir_all(dir.path().join("ignored/src")).unwrap();
        std::fs::write(dir.path().join("ignored/src").join(source_name), source).unwrap();
        std::fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
        assert!(walk_supported_files_under(
            dir.path(),
            &ExcludeMatcher::empty(),
            Some(Path::new("ignored")),
        )
        .is_empty());

        #[cfg(unix)]
        {
            std::fs::create_dir_all(dir.path().join(".hidden/src")).unwrap();
            std::fs::write(dir.path().join(".hidden/src").join(source_name), source).unwrap();
            std::os::unix::fs::symlink(".hidden", dir.path().join("linked")).unwrap();
            assert!(workspace_is_within(dir.path(), Path::new("linked")));
            assert!(walk_supported_files_under(
                dir.path(),
                &ExcludeMatcher::empty(),
                Some(Path::new("linked")),
            )
            .is_empty());
        }
    }
}
