//! Post-commit git hook script + install logic.
//!
//! Lives in its own module — separable from the broader `init` flow and
//! sibling to `commands::hook` (which runs as the script written here).
//!
//! The script invokes `heal hook commit || true`. The `|| true`, the
//! `command -v heal` guard, and the `# heal post-commit hook` marker are
//! load-bearing — see `.claude/rules/skills-and-hooks.md` R2 before
//! editing.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::observer::git;

pub(crate) const HEAL_HOOK_MARKER: &str = "# heal post-commit hook";
pub(crate) const POST_COMMIT_SCRIPT: &str = "\
#!/usr/bin/env sh
# heal post-commit hook
# Re-runs observers and emits the post-commit nudge.
# Failures are swallowed so a broken HEAL install never blocks a commit.
if command -v heal >/dev/null 2>&1; then
  heal hook commit || true
fi
exit 0
";

/// Internally tagged so it flattens safely under `path:` in the JSON
/// contract emitted by `heal init --json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum HookAction {
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

pub(crate) fn install(project: &Path, force: bool) -> Result<(HookAction, Option<PathBuf>)> {
    let Some(hooks_dir) = git::hooks_dir(project) else {
        return Ok((HookAction::SkippedNoRepo, None));
    };
    std::fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("creating {}", hooks_dir.display()))?;
    let hook_path = hooks_dir.join("post-commit");

    if hook_path.exists() {
        let body = std::fs::read_to_string(&hook_path).unwrap_or_default();
        if body.contains(HEAL_HOOK_MARKER) {
            write_hook(&hook_path)?;
            return Ok((HookAction::Refreshed, Some(hook_path)));
        }
        if !force {
            return Ok((HookAction::SkippedUserHook, Some(hook_path)));
        }
        write_hook(&hook_path)?;
        return Ok((HookAction::Overwrote, Some(hook_path)));
    }

    write_hook(&hook_path)?;
    Ok((HookAction::Installed, Some(hook_path)))
}

fn write_hook(hook_path: &Path) -> Result<()> {
    let already_current =
        std::fs::read_to_string(hook_path).is_ok_and(|prior| prior == POST_COMMIT_SCRIPT);
    if !already_current {
        std::fs::write(hook_path, POST_COMMIT_SCRIPT)
            .with_context(|| format!("writing {}", hook_path.display()))?;
    }
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

#[cfg(test)]
pub(crate) fn hook_path_for(project: &Path) -> PathBuf {
    git::hooks_dir(project)
        .expect("test repo must be initialized before requesting hook path")
        .join("post-commit")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::init_repo;
    use tempfile::TempDir;

    #[test]
    fn install_skips_outside_git_repo() {
        let dir = TempDir::new().unwrap();
        let (action, path) = install(dir.path(), false).unwrap();
        assert_eq!(action, HookAction::SkippedNoRepo);
        assert!(path.is_none(), "hook path is meaningless without a repo");
    }

    #[cfg(unix)]
    #[test]
    fn install_writes_executable_post_commit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let (action, path) = install(dir.path(), false).unwrap();
        assert_eq!(action, HookAction::Installed);
        let hook = path.expect("hook path must be returned on a real repo");
        assert_eq!(hook, hook_path_for(dir.path()));
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
    fn install_refreshes_own_marker() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let hook = hook_path_for(dir.path());
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(
            &hook,
            format!("#!/bin/sh\n{HEAL_HOOK_MARKER}\necho stale\n"),
        )
        .unwrap();
        let (action, path) = install(dir.path(), false).unwrap();
        assert_eq!(action, HookAction::Refreshed);
        assert_eq!(path.as_deref(), Some(hook.as_path()));
        let body = std::fs::read_to_string(&hook).unwrap();
        assert!(body.contains("heal hook commit"));
        assert!(!body.contains("stale"));
    }

    #[test]
    fn install_preserves_user_hook_without_force() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let hook = hook_path_for(dir.path());
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(&hook, "#!/bin/sh\necho user hook\n").unwrap();
        let (action, _) = install(dir.path(), false).unwrap();
        assert_eq!(action, HookAction::SkippedUserHook);
        let body = std::fs::read_to_string(&hook).unwrap();
        assert!(body.contains("echo user hook"));
    }

    #[test]
    fn install_overwrites_user_hook_with_force() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let hook = hook_path_for(dir.path());
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(&hook, "#!/bin/sh\necho user hook\n").unwrap();
        let (action, _) = install(dir.path(), true).unwrap();
        assert_eq!(action, HookAction::Overwrote);
        let body = std::fs::read_to_string(&hook).unwrap();
        assert!(body.contains(HEAL_HOOK_MARKER));
    }
}
