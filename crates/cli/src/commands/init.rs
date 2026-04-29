//! `heal init` — wire HEAL into a project.
//!
//! Steps, in order:
//!   1. Ensure `.heal/` layout exists.
//!   2. Detect the primary language via `LocObserver` for the user-facing
//!      summary (not persisted — `heal status` re-detects on every call).
//!   3. Write a default `config.toml` (skipped when one already exists
//!      unless `--force`).
//!   4. Install a `post-commit` git hook that calls `heal hook commit`.
//!   5. Run an initial scan and append it to `snapshots/` as an `init`
//!      event so `heal status` has something to compare against.

use std::fmt;
use std::path::Path;

use anyhow::{Context, Result};
use heal_core::config::Config;
use heal_core::eventlog::{Event, EventLog};
use heal_core::HealPaths;
use heal_observer::git;
use heal_observer::loc::LocObserver;

use crate::snapshot;

const HEAL_HOOK_MARKER: &str = "# heal post-commit hook";
const POST_COMMIT_SCRIPT: &str = "\
#!/usr/bin/env sh
# heal post-commit hook
# Records a MetricsSnapshot to .heal/snapshots/YYYY-MM.jsonl plus a
# CommitInfo entry to .heal/logs/YYYY-MM.jsonl after each commit.
# Failures are swallowed so a broken HEAL install never blocks a commit.
if command -v heal >/dev/null 2>&1; then
  heal hook commit || true
fi
exit 0
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigAction {
    Wrote,
    Overwrote,
    KeptExisting,
}

impl fmt::Display for ConfigAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Wrote => "wrote",
            Self::Overwrote => "overwrote",
            Self::KeptExisting => "kept existing",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookAction {
    Installed,
    Overwrote,
    Refreshed,
    SkippedNoRepo,
    SkippedUserHook,
}

impl fmt::Display for HookAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Installed => "installed",
            Self::Overwrote => "overwrote",
            Self::Refreshed => "refreshed",
            Self::SkippedNoRepo => "skipped (not a git repo)",
            Self::SkippedUserHook => {
                "skipped (existing user hook; rerun with --force to overwrite)"
            }
        })
    }
}

pub fn run(project: &Path, force: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    paths
        .ensure()
        .with_context(|| format!("creating {}", paths.root().display()))?;

    let primary_language = LocObserver::default().scan(project).primary;
    let config_action = write_config(&paths, force)?;
    let hook_action = install_post_commit_hook(project, force)?;
    run_initial_scan(project, &paths)?;

    println!("HEAL initialized at {}", paths.root().display());
    if let Some(lang) = primary_language.as_deref() {
        println!("  primary language: {lang}");
    } else {
        println!("  primary language: (not detected)");
    }
    println!(
        "  config:           {} ({config_action})",
        paths.config().display(),
    );
    println!("  post-commit hook: {hook_action}");
    println!("  initial snapshot: captured");
    println!("next steps:");
    println!("  1. heal skills install   # install Claude plugin");
    println!("  2. heal status           # see current findings");
    Ok(())
}

fn write_config(paths: &HealPaths, force: bool) -> Result<ConfigAction> {
    let cfg_path = paths.config();
    let already_present = cfg_path.exists();
    if already_present && !force {
        return Ok(ConfigAction::KeptExisting);
    }
    Config::default().save(&cfg_path)?;
    Ok(if already_present {
        ConfigAction::Overwrote
    } else {
        ConfigAction::Wrote
    })
}

fn install_post_commit_hook(project: &Path, force: bool) -> Result<HookAction> {
    let Some(hooks_dir) = git::hooks_dir(project) else {
        return Ok(HookAction::SkippedNoRepo);
    };
    std::fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("creating {}", hooks_dir.display()))?;
    let hook_path = hooks_dir.join("post-commit");

    if hook_path.exists() {
        let body = std::fs::read_to_string(&hook_path).unwrap_or_default();
        if body.contains(HEAL_HOOK_MARKER) {
            write_hook(&hook_path)?;
            return Ok(HookAction::Refreshed);
        }
        if !force {
            return Ok(HookAction::SkippedUserHook);
        }
        write_hook(&hook_path)?;
        return Ok(HookAction::Overwrote);
    }

    write_hook(&hook_path)?;
    Ok(HookAction::Installed)
}

fn write_hook(hook_path: &Path) -> Result<()> {
    std::fs::write(hook_path, POST_COMMIT_SCRIPT)
        .with_context(|| format!("writing {}", hook_path.display()))?;
    set_executable(hook_path)?;
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(path)
        .with_context(|| format!("stat {}", path.display()))?
        .permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(path, perm).with_context(|| format!("chmod {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn run_initial_scan(project: &Path, paths: &HealPaths) -> Result<()> {
    let payload = snapshot::capture_value(project)?;
    EventLog::new(paths.snapshots_dir()).append(&Event::new("init", payload))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{commit, init_repo};
    use tempfile::TempDir;

    fn commit_default(cwd: &Path, file: &str, body: &str, email: &str) {
        commit(cwd, file, body, email, "snap");
    }

    fn hook_path(project: &Path) -> std::path::PathBuf {
        git::hooks_dir(project)
            .expect("test repo must be initialized before requesting hook path")
            .join("post-commit")
    }

    #[test]
    fn write_config_writes_default_when_absent() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        let action = write_config(&paths, false).unwrap();
        assert_eq!(action, ConfigAction::Wrote);
        let cfg = Config::load(&paths.config()).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn write_config_keeps_existing_without_force() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        std::fs::write(paths.config(), "# user-edited\n").unwrap();
        let action = write_config(&paths, false).unwrap();
        assert_eq!(action, ConfigAction::KeptExisting);
        let body = std::fs::read_to_string(paths.config()).unwrap();
        assert_eq!(body, "# user-edited\n");
    }

    #[test]
    fn write_config_overwrites_with_force() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        std::fs::write(paths.config(), "# user-edited\n").unwrap();
        let action = write_config(&paths, true).unwrap();
        assert_eq!(action, ConfigAction::Overwrote);
        let cfg = Config::load(&paths.config()).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn install_hook_skips_outside_git_repo() {
        let dir = TempDir::new().unwrap();
        let action = install_post_commit_hook(dir.path(), false).unwrap();
        assert_eq!(action, HookAction::SkippedNoRepo);
    }

    #[cfg(unix)]
    #[test]
    fn install_hook_writes_executable_post_commit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let action = install_post_commit_hook(dir.path(), false).unwrap();
        assert_eq!(action, HookAction::Installed);
        let hook = hook_path(dir.path());
        let body = std::fs::read_to_string(&hook).unwrap();
        assert!(body.contains(HEAL_HOOK_MARKER));
        assert!(body.contains("heal hook commit"));
        let mode = std::fs::metadata(&hook).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o111,
            0o111,
            "hook must be executable; mode={mode:o}"
        );
    }

    #[test]
    fn install_hook_refreshes_own_marker() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let hook = hook_path(dir.path());
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(
            &hook,
            format!("#!/bin/sh\n{HEAL_HOOK_MARKER}\necho stale\n"),
        )
        .unwrap();
        let action = install_post_commit_hook(dir.path(), false).unwrap();
        assert_eq!(action, HookAction::Refreshed);
        let body = std::fs::read_to_string(&hook).unwrap();
        assert!(body.contains("heal hook commit"));
        assert!(!body.contains("stale"));
    }

    #[test]
    fn install_hook_preserves_user_hook_without_force() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let hook = hook_path(dir.path());
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(&hook, "#!/bin/sh\necho user hook\n").unwrap();
        let action = install_post_commit_hook(dir.path(), false).unwrap();
        assert_eq!(action, HookAction::SkippedUserHook);
        let body = std::fs::read_to_string(&hook).unwrap();
        assert!(body.contains("echo user hook"));
    }

    #[test]
    fn install_hook_overwrites_user_hook_with_force() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let hook = hook_path(dir.path());
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(&hook, "#!/bin/sh\necho user hook\n").unwrap();
        let action = install_post_commit_hook(dir.path(), true).unwrap();
        assert_eq!(action, HookAction::Overwrote);
        let body = std::fs::read_to_string(&hook).unwrap();
        assert!(body.contains(HEAL_HOOK_MARKER));
    }

    #[test]
    fn run_end_to_end_creates_layout_config_and_snapshot() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_default(dir.path(), "main.rs", "fn main() {}\n", "solo@example.com");
        run(dir.path(), false).unwrap();
        let paths = HealPaths::new(dir.path());
        assert!(paths.config().exists(), "config.toml must exist");
        assert!(paths.snapshots_dir().exists(), "snapshots dir must exist");
        let any_snapshot = std::fs::read_dir(paths.snapshots_dir())
            .unwrap()
            .any(|e| e.is_ok());
        assert!(any_snapshot, "snapshots dir must contain the init record");
        assert!(
            hook_path(dir.path()).exists(),
            "post-commit hook must be installed",
        );
    }
}
