//! Helpers shared across the observer crate's integration tests.
//!
//! Cargo treats each file directly under `tests/` as its own integration
//! binary; modules under `tests/common/` are pulled in via `mod common;` in
//! each binary that needs them and don't compile as a standalone test.

#![allow(dead_code)] // helpers used by some test binaries, not all

use std::fs;
use std::path::Path;

use git2::{IndexAddOption, Repository, Signature, Time};

/// Write `body` to `root.join(rel)`, creating intermediate directories.
/// Panics on I/O failure — appropriate inside `#[test]` fixtures.
pub fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

/// Initialize a git repository at `root` with deterministic identity.
pub fn init_repo(root: &Path) -> Repository {
    let repo = Repository::init(root).expect("git init");
    let mut config = repo.config().expect("repo config");
    config.set_str("user.name", "HEAL Test").unwrap();
    config.set_str("user.email", "test@heal.local").unwrap();
    repo
}

/// Write `files` into `root` and create one commit containing exactly those
/// files (staged via `git add -A`). `time_secs` is the unix timestamp used
/// for both author and committer time, so churn windows are reproducible.
pub fn commit_files(repo: &Repository, files: &[(&str, &str)], message: &str, time_secs: i64) {
    let workdir = repo.workdir().expect("non-bare repo").to_path_buf();
    for (rel, body) in files {
        write(&workdir, rel, body);
    }
    let mut index = repo.index().expect("repo index");
    index
        .add_all(["*"], IndexAddOption::DEFAULT, None)
        .expect("git add");
    index.write().expect("write index");
    let tree_oid = index.write_tree().expect("write tree");
    let tree = repo.find_tree(tree_oid).expect("find tree");

    let sig = Signature::new("HEAL Test", "test@heal.local", &Time::new(time_secs, 0))
        .expect("signature");

    let parents: Vec<git2::Commit<'_>> = match repo.head() {
        Ok(h) => vec![h.peel_to_commit().expect("head commit")],
        Err(_) => Vec::new(),
    };
    let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();

    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
        .expect("commit");
}
