//! Small `git2` helpers shared across observers.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use git2::{Repository, Sort};

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

/// Count distinct author emails reachable from HEAD, walking at most
/// `max_walk` commits and stopping early once `stop_at` distinct emails
/// have been seen. Returns `0` for non-repos or unborn HEAD so the caller
/// can treat the result uniformly. Email comparison is case-insensitive;
/// commits without an email are ignored. Pass `usize::MAX` for `stop_at`
/// to disable the short-circuit.
#[must_use]
pub fn distinct_author_emails(root: &Path, max_walk: usize, stop_at: usize) -> usize {
    let Ok(repo) = Repository::discover(root) else {
        return 0;
    };
    let Ok(mut walk) = repo.revwalk() else {
        return 0;
    };
    if walk.set_sorting(Sort::TIME).is_err() || walk.push_head().is_err() {
        return 0;
    }
    let mut emails: HashSet<String> = HashSet::new();
    for oid_res in walk.take(max_walk) {
        let Ok(oid) = oid_res else {
            continue;
        };
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        let author = commit.author();
        if let Some(email) = author.email() {
            emails.insert(email.to_lowercase());
        }
        if emails.len() >= stop_at {
            break;
        }
    }
    emails.len()
}

/// Locate the `hooks/` directory of the git repository containing `root`.
/// Returns `None` when `root` isn't inside a git repo. Uses the common
/// gitdir so worktrees install hooks alongside the main repo's hooks.
#[must_use]
pub fn hooks_dir(root: &Path) -> Option<PathBuf> {
    let repo = Repository::discover(root).ok()?;
    Some(repo.commondir().join("hooks"))
}
