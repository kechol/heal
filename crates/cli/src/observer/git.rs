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

/// True iff the working tree has no uncommitted changes (no untracked,
/// modified, staged, or conflicted entries). `None` when `root` isn't a
/// git repo — callers (`heal check` cache layer) treat that as "can't
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
