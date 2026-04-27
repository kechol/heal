//! Small `git2` helpers shared across observers.

use std::path::Path;

/// Best-effort HEAD SHA lookup. Returns `None` when `root` isn't inside a
/// git repo or HEAD is unborn (e.g. fresh `git init` before the first
/// commit).
#[must_use]
pub fn head_sha(root: &Path) -> Option<String> {
    let repo = git2::Repository::discover(root).ok()?;
    let head = repo.head().ok()?;
    let oid = head.target()?;
    Some(oid.to_string())
}
