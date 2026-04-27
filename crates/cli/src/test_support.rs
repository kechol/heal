//! Shared `git`-shelling test helpers used by command-level inline tests.
//! Centralized so adding a new hook test doesn't reinvent the same setup
//! each time. `cfg(test)` so this never ships in release builds.

#![cfg(test)]

use std::path::Path;
use std::process::Command;

pub(crate) fn git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .expect("git available on $PATH");
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
