//! Shared `git`-shelling test helpers used by command-level inline tests.
//! Centralized so adding a new hook test doesn't reinvent the same setup
//! each time. `cfg(test)` so this never ships in release builds.

#![cfg(test)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Resolve `git`'s absolute path once and cache it. Other inline tests
/// (`init::tests::handle_skills_install_*`) mutate `PATH` to drive the
/// `claude` lookup logic, which races with `Command::new("git")`'s
/// runtime PATH resolution under cargo's parallel test scheduler. Pinning
/// the binary up front makes git-shelling tests immune to that race.
fn git_bin() -> &'static Path {
    static CACHED: OnceLock<PathBuf> = OnceLock::new();
    CACHED.get_or_init(|| {
        if let Some(path_var) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let candidate = dir.join("git");
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
        for fallback in [
            "/usr/bin/git",
            "/usr/local/bin/git",
            "/opt/homebrew/bin/git",
        ] {
            let p = PathBuf::from(fallback);
            if p.is_file() {
                return p;
            }
        }
        panic!("git not found on $PATH or in common system locations");
    })
}

pub(crate) fn git(cwd: &Path, args: &[&str]) {
    let status = Command::new(git_bin())
        .args(args)
        .current_dir(cwd)
        .status()
        .expect("invoking git");
    assert!(status.success(), "git {args:?} failed in {}", cwd.display());
}

pub(crate) fn init_repo(cwd: &Path) {
    git(cwd, &["init", "-q"]);
    git(cwd, &["config", "commit.gpgsign", "false"]);
}

/// Stage `file` with `body`, then commit as `email` with subject `msg`.
pub(crate) fn commit(cwd: &Path, file: &str, body: &str, email: &str, msg: &str) {
    std::fs::write(cwd.join(file), body).unwrap();
    git(cwd, &["add", file]);
    git(
        cwd,
        &[
            "-c",
            &format!("user.email={email}"),
            "-c",
            "user.name=tester",
            "commit",
            "-q",
            "-m",
            msg,
        ],
    );
}
