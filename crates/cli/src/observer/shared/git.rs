//! Small `git2` helpers shared across observers.

use std::path::{Path, PathBuf};

use git2::Repository;
use serde::{Deserialize, Serialize};

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
        author_email: author.email().map(str::to_string),
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
