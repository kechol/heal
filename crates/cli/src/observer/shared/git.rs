//! Small `git2` helpers shared across observers.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use git2::{Oid, Patch, Repository, Sort};
use serde::{Deserialize, Serialize};

use super::walk::{path_under, resolve_workspace_target, since_cutoff, ExcludeMatcher};

#[derive(Debug, Clone)]
pub(crate) struct HistoryCommit {
    pub oid: Oid,
    pub paths: Vec<PathBuf>,
    pub line_stats: Vec<(PathBuf, u32, u32)>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct History {
    pub commits: Vec<HistoryCommit>,
    #[cfg(test)]
    pub diffs_generated: usize,
    #[cfg(test)]
    pub patches_generated: usize,
}

pub(crate) fn collect_history(
    root: &Path,
    since_days: u32,
    excluded: &[String],
    workspace: Option<&Path>,
    include_line_stats: bool,
) -> History {
    let Ok(repo) = Repository::discover(root) else {
        return History::default();
    };
    let Ok(head_commit) = repo.head().and_then(|head| head.peel_to_commit()) else {
        return History::default();
    };
    let cutoff_secs = since_cutoff(head_commit.time().seconds(), since_days);
    let Ok(mut revwalk) = repo.revwalk() else {
        return History::default();
    };
    if revwalk.set_sorting(Sort::TIME).is_err() || revwalk.push_head().is_err() {
        return History::default();
    }
    let workspace_target = resolve_workspace_target(root, workspace, false);
    let matcher =
        ExcludeMatcher::compile(root, excluded).expect("exclude patterns validated at config load");
    let mut history = History::default();
    for oid in revwalk.filter_map(Result::ok) {
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        if commit.time().seconds() < cutoff_secs {
            break;
        }
        let Ok(commit_tree) = commit.tree() else {
            continue;
        };
        let parent_tree = commit.parent(0).ok().and_then(|parent| parent.tree().ok());
        let Ok(diff) = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)
        else {
            continue;
        };
        #[cfg(test)]
        {
            history.diffs_generated += 1;
        }
        let mut paths = BTreeSet::new();
        let mut line_stats = Vec::new();
        for (index, delta) in diff.deltas().enumerate() {
            let Some(path) = delta.new_file().path() else {
                continue;
            };
            if path.as_os_str().is_empty()
                || !path_under(path, workspace_target.as_deref())
                || matcher.is_excluded(path, false)
            {
                continue;
            }
            paths.insert(path.to_path_buf());
            if include_line_stats {
                #[cfg(test)]
                {
                    history.patches_generated += 1;
                }
                let (added, deleted) = Patch::from_diff(&diff, index)
                    .ok()
                    .flatten()
                    .and_then(|patch| patch.line_stats().ok())
                    .map_or((0, 0), |(_, added, deleted)| (added, deleted));
                line_stats.push((
                    path.to_path_buf(),
                    u32::try_from(added).unwrap_or(u32::MAX),
                    u32::try_from(deleted).unwrap_or(u32::MAX),
                ));
            }
        }
        history.commits.push(HistoryCommit {
            oid,
            paths: paths.into_iter().collect(),
            line_stats,
        });
    }
    history
}

/// Best-effort HEAD SHA lookup. Returns `None` when `root` isn't inside a
/// git repo or HEAD is unborn (e.g. fresh `git init` before the first
/// commit).
#[must_use]
pub fn head_sha(root: &Path) -> Option<String> {
    let repo = Repository::discover(root).ok()?;
    let head = repo.head().ok()?;
    let oid = head.target()?;
    Some(oid.to_string())
}

/// Resolve a user-supplied git revision (`HEAD`, `main`, `v0.2.1`,
/// `HEAD~3`, or a partial / full SHA) to a full 40-char object id.
/// Returns `None` when `root` isn't a git repo or `revspec` doesn't
/// resolve. Wraps `git rev-parse <revspec>` semantics via `git2`.
#[must_use]
pub fn resolve_ref(root: &Path, revspec: &str) -> Option<String> {
    let repo = Repository::discover(root).ok()?;
    let object = repo.revparse_single(revspec).ok()?;
    Some(object.id().to_string())
}

/// Number of commits reachable from HEAD but not from `since_sha`
/// (`git rev-list <since_sha>..HEAD --count`). `None` when `root` isn't a
/// git repo, HEAD is unborn, or `since_sha` no longer resolves (e.g. the
/// commit was rewritten away).
#[must_use]
pub fn commits_since(root: &Path, since_sha: &str) -> Option<usize> {
    let repo = Repository::discover(root).ok()?;
    let since = repo.revparse_single(since_sha).ok()?.id();
    let mut walk = repo.revwalk().ok()?;
    walk.push_head().ok()?;
    walk.hide(since).ok()?;
    Some(walk.flatten().count())
}

/// True iff the working tree has no uncommitted changes (no untracked,
/// modified, staged, or conflicted entries). `None` when `root` isn't a
/// git repo — callers (`heal status` cache layer) treat that as "can't
/// claim cleanliness, so don't reuse a clean cache".
#[must_use]
pub fn worktree_clean(root: &Path) -> Option<bool> {
    let repo = Repository::discover(root).ok()?;
    let mut opts = git2::StatusOptions::new();
    // `include_ignored = false` is the default; we mirror `git status` —
    // untracked counts as dirty so a half-applied refactor isn't
    // miscategorised as a reusable clean check.
    opts.include_untracked(true);
    opts.include_ignored(false);
    let statuses = repo.statuses(Some(&mut opts)).ok()?;
    Some(statuses.is_empty())
}

/// `Name <email>` from `git config user.{name,email}` (merged repo +
/// global view), best-effort. Returns `None` when either component is
/// missing or `root` isn't inside a git repo. Used by `heal mark
/// accept` for the `accepted_by` audit-trail snapshot — falling back
/// to `None` keeps the command working in CI / detached configs.
#[must_use]
pub fn user_signature(root: &Path) -> Option<String> {
    let repo = Repository::discover(root).ok()?;
    let cfg = repo.config().ok()?;
    let name = cfg.get_string("user.name").ok()?;
    let email = cfg.get_string("user.email").ok()?;
    Some(format!("{name} <{email}>"))
}

/// Locate the `hooks/` directory of the git repository containing `root`.
/// Returns `None` when `root` isn't inside a git repo. Uses the common
/// gitdir so worktrees install hooks alongside the main repo's hooks.
#[must_use]
pub fn hooks_dir(root: &Path) -> Option<PathBuf> {
    let repo = Repository::discover(root).ok()?;
    Some(repo.commondir().join("hooks"))
}

/// Lightweight HEAD commit summary used as the `logs/` payload for
/// post-commit events. Captures who committed, what the message says, and a
/// rough size of the change. Counts are derived from a first-parent diff so
/// merge commits aren't double-counted; the root commit reports zero diff
/// stats since there's no parent to diff against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitInfo {
    pub sha: String,
    pub parent_sha: Option<String>,
    pub author_email: Option<String>,
    /// First line of the commit message (`subject`).
    pub message_summary: String,
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
}

/// Read HEAD's commit metadata. Returns `None` when `root` isn't a git repo
/// or HEAD is unborn.
#[must_use]
pub fn head_commit_info(root: &Path) -> Option<CommitInfo> {
    let repo = Repository::discover(root).ok()?;
    let head = repo.head().ok()?;
    let oid = head.target()?;
    let commit = repo.find_commit(oid).ok()?;

    let author = commit.author();
    let message = commit.message().unwrap_or("");
    let message_summary = message.lines().next().unwrap_or("").to_string();
    let parent = commit.parent(0).ok();
    let parent_sha = parent.as_ref().map(|c| c.id().to_string());

    // Root commits (no parent) diff against an empty tree so the very first
    // commit still reports its own additions instead of `(0, 0, 0)`. Counts
    // saturate at u32::MAX — a commit large enough to overflow already
    // means whatever number we report is "absurd", so capping is safer than
    // panicking on the cast.
    let parent_tree = parent.as_ref().and_then(|p| p.tree().ok());
    let head_tree = commit.tree().ok();
    let (files_changed, insertions, deletions) = repo
        .diff_tree_to_tree(parent_tree.as_ref(), head_tree.as_ref(), None)
        .ok()
        .and_then(|d| d.stats().ok())
        .map_or((0, 0, 0), |s| {
            (
                u32::try_from(s.files_changed()).unwrap_or(u32::MAX),
                u32::try_from(s.insertions()).unwrap_or(u32::MAX),
                u32::try_from(s.deletions()).unwrap_or(u32::MAX),
            )
        });

    Some(CommitInfo {
        sha: oid.to_string(),
        parent_sha,
        // git2 0.21 returns `Result<&str, _>` (Err on non-UTF-8),
        // where 0.20 returned `Option<&str>` — same "absent" semantics.
        author_email: author.email().ok().map(str::to_string),
        message_summary,
        files_changed,
        insertions,
        deletions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{commit, git, init_repo};
    use tempfile::TempDir;

    fn set_user(dir: &Path) {
        git(dir, &["config", "user.name", "tester"]);
        git(dir, &["config", "user.email", "tester@example.com"]);
    }

    #[test]
    fn history_generates_patches_only_for_selected_deltas() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::create_dir_all(dir.path().join("vendor")).unwrap();
        std::fs::write(dir.path().join("src/keep.rs"), "fn keep() {}\n").unwrap();
        std::fs::write(dir.path().join("vendor/drop.rs"), "fn drop() {}\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(
            dir.path(),
            &[
                "-c",
                "user.email=tester@example.com",
                "-c",
                "user.name=tester",
                "commit",
                "-q",
                "-m",
                "fixture",
            ],
        );

        let history = collect_history(dir.path(), 90, &["vendor/".into()], None, true);
        assert_eq!(history.diffs_generated, 1);
        assert_eq!(history.patches_generated, 1);
        assert_eq!(history.commits[0].paths, vec![PathBuf::from("src/keep.rs")]);
        assert_eq!(
            history.commits[0].line_stats,
            vec![(PathBuf::from("src/keep.rs"), 1, 0)],
        );

        let paths_only = collect_history(dir.path(), 90, &[], None, false);
        assert_eq!(paths_only.diffs_generated, 1);
        assert_eq!(paths_only.patches_generated, 0);
        assert!(paths_only.commits[0].line_stats.is_empty());
    }

    // ── head_sha ────────────────────────────────────────────────────

    #[test]
    fn head_sha_returns_none_outside_repo() {
        let dir = TempDir::new().unwrap();
        // Plain tempdir, no `git init` — `Repository::discover` fails.
        assert!(head_sha(dir.path()).is_none());
    }

    #[test]
    fn head_sha_returns_none_on_unborn_head() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        // Fresh `git init` with no commits — HEAD points at a ref that
        // doesn't exist yet. `repo.head()` errors here.
        assert!(head_sha(dir.path()).is_none());
    }

    #[test]
    fn head_sha_returns_full_oid_after_commit() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "a.txt", "x\n", "tester@example.com", "init");
        let sha = head_sha(dir.path()).expect("HEAD must resolve after commit");
        assert_eq!(sha.len(), 40, "sha must be full 40-hex");
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ── resolve_ref ────────────────────────────────────────────────

    #[test]
    fn resolve_ref_returns_none_outside_repo() {
        let dir = TempDir::new().unwrap();
        assert!(resolve_ref(dir.path(), "HEAD").is_none());
    }

    #[test]
    fn resolve_ref_returns_none_for_unresolvable_revspec() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "a.txt", "x\n", "tester@example.com", "init");
        assert!(resolve_ref(dir.path(), "no-such-branch-or-tag").is_none());
    }

    #[test]
    fn resolve_ref_resolves_head_and_branch_to_oid() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "a.txt", "x\n", "tester@example.com", "init");
        let head = head_sha(dir.path()).unwrap();
        assert_eq!(
            resolve_ref(dir.path(), "HEAD").as_deref(),
            Some(head.as_str())
        );
    }

    // ── worktree_clean ─────────────────────────────────────────────

    #[test]
    fn worktree_clean_returns_none_outside_repo() {
        let dir = TempDir::new().unwrap();
        assert!(worktree_clean(dir.path()).is_none());
    }

    #[test]
    fn worktree_clean_true_after_clean_commit() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "a.txt", "x\n", "tester@example.com", "init");
        assert_eq!(worktree_clean(dir.path()), Some(true));
    }

    #[test]
    fn worktree_clean_false_when_untracked_file_present() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "a.txt", "x\n", "tester@example.com", "init");
        std::fs::write(dir.path().join("untracked.txt"), "hi\n").unwrap();
        // Untracked must count as dirty so a half-applied refactor isn't
        // miscategorised as a reusable clean check.
        assert_eq!(worktree_clean(dir.path()), Some(false));
    }

    #[test]
    fn worktree_clean_false_when_tracked_file_modified() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "a.txt", "x\n", "tester@example.com", "init");
        std::fs::write(dir.path().join("a.txt"), "y\n").unwrap();
        assert_eq!(worktree_clean(dir.path()), Some(false));
    }

    // ── user_signature ─────────────────────────────────────────────

    #[test]
    fn user_signature_returns_none_outside_repo() {
        let dir = TempDir::new().unwrap();
        assert!(user_signature(dir.path()).is_none());
    }

    #[test]
    fn user_signature_formats_name_and_email_when_both_set() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        set_user(dir.path());
        assert_eq!(
            user_signature(dir.path()),
            Some("tester <tester@example.com>".into()),
        );
    }

    // ── hooks_dir ──────────────────────────────────────────────────

    #[test]
    fn hooks_dir_returns_none_outside_repo() {
        let dir = TempDir::new().unwrap();
        assert!(hooks_dir(dir.path()).is_none());
    }

    #[test]
    fn hooks_dir_resolves_to_gitdir_hooks() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let h = hooks_dir(dir.path()).expect("hooks dir must resolve");
        // `commondir()` ends with `.git/` for a non-worktree repo; the
        // hooks dir is therefore `<...>/.git/hooks`. We only assert the
        // suffix because macOS `TempDir` paths under `/var/` resolve to
        // `/private/var/` after canonicalisation, which would break a
        // `starts_with(dir.path())` check.
        assert!(
            h.ends_with("hooks"),
            "expected `.../hooks`, got {}",
            h.display()
        );
        assert!(h.parent().unwrap().ends_with(".git"));
    }

    // ── head_commit_info ───────────────────────────────────────────

    #[test]
    fn head_commit_info_returns_none_outside_repo() {
        let dir = TempDir::new().unwrap();
        assert!(head_commit_info(dir.path()).is_none());
    }

    #[test]
    fn head_commit_info_returns_none_on_unborn_head() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        assert!(head_commit_info(dir.path()).is_none());
    }

    #[test]
    fn head_commit_info_root_commit_has_no_parent_and_reports_own_stats() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(
            dir.path(),
            "a.txt",
            "line one\nline two\n",
            "alice@example.com",
            "first commit",
        );
        let info = head_commit_info(dir.path()).expect("head commit must resolve");
        assert!(info.parent_sha.is_none(), "root commit has no parent");
        assert_eq!(info.author_email.as_deref(), Some("alice@example.com"));
        assert_eq!(info.message_summary, "first commit");
        // Root-commit diff is taken against an empty tree so additions
        // for the very first commit still surface (otherwise post-commit
        // log payloads would report `(0, 0, 0)` for fresh repos).
        assert_eq!(info.files_changed, 1);
        assert_eq!(info.insertions, 2);
        assert_eq!(info.deletions, 0);
    }

    #[test]
    fn head_commit_info_with_parent_records_parent_sha_and_first_parent_diff() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "a.txt", "one\n", "alice@example.com", "first");
        let parent = head_sha(dir.path()).unwrap();
        commit(
            dir.path(),
            "a.txt",
            "one\ntwo\nthree\n",
            "bob@example.com",
            "second commit",
        );
        let info = head_commit_info(dir.path()).expect("head commit must resolve");
        assert_eq!(info.parent_sha.as_deref(), Some(parent.as_str()));
        assert_eq!(info.author_email.as_deref(), Some("bob@example.com"));
        assert_eq!(info.message_summary, "second commit");
        assert_eq!(info.files_changed, 1);
        assert_eq!(info.insertions, 2);
        assert_eq!(info.deletions, 0);
    }

    #[test]
    fn head_commit_info_message_summary_is_first_line_only() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit(
            dir.path(),
            "a.txt",
            "x\n",
            "tester@example.com",
            "subject line\n\nbody paragraph that must NOT leak into summary",
        );
        let info = head_commit_info(dir.path()).unwrap();
        assert_eq!(info.message_summary, "subject line");
    }
}
