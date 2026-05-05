//! Shared `git`-shelling test helpers used by command-level inline tests.
//! Centralized so adding a new hook test doesn't reinvent the same setup
//! each time. `cfg(test)` so this never ships in release builds.

#![cfg(test)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::core::config::Config;
use crate::core::HealPaths;

/// Resolve `git`'s absolute path once and cache it. The cache exists
/// because `Command::new("git")` resolves `PATH` at runtime, and any
/// inline test that mutates `PATH` (we no longer have one in
/// `commands/init.rs`, but the helper is kept defensive) would race
/// against parallel git-shelling siblings. Pinning the binary up front
/// makes git-shelling tests immune to that race.
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

/// Initialize a tempdir as a one-commit git repo with `lib.rs` containing
/// `source`, then materialize `.heal/` and persist a default `Config`.
/// Returns the resolved [`HealPaths`] so callers can target
/// `.heal/config.toml` / `.heal/findings/` without rederiving them.
///
/// Replaces the per-module `init_project` helpers that previously
/// duplicated this exact sequence across `commands/calibrate.rs`,
/// `commands/hook.rs`, and `commands/metrics/mod.rs` tests.
pub(crate) fn init_project_with_config(dir: &Path, source: &str) -> HealPaths {
    init_repo(dir);
    commit(dir, "lib.rs", source, "tester@example.com", "init");
    let paths = HealPaths::new(dir);
    paths.ensure().unwrap();
    Config::default().save(&paths.config()).unwrap();
    paths
}
