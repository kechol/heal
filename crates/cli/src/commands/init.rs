//! `heal init` — wire HEAL into a project.
//!
//! Steps, in order:
//!   1. Ensure `.heal/` layout exists.
//!   2. Detect the primary language via `LocObserver` for the user-facing
//!      summary (not persisted — `heal status` re-detects on every call).
//!   3. Write a default `config.toml` (skipped when one already exists
//!      unless `--force`).
//!   4. Install a `post-commit` git hook that calls `heal hook commit`.
//!   5. Run an initial scan, derive `.heal/calibration.toml` from its
//!      distribution, then append a `MetricsSnapshot` (with
//!      `severity_counts` already classified by the new calibration)
//!      to `snapshots/` as an `init` event.
//!   6. Optionally extract the Claude plugin to `.claude/plugins/heal/`
//!      (prompted when `claude` is on `PATH` and stdin is a TTY; bypassed
//!      with `--yes` / `--no-skills`).

use std::fmt;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::claude_settings;
use crate::core::config::Config;
use crate::core::eventlog::{Event, EventLog};
use crate::core::snapshot::SeverityCounts;
use crate::core::HealPaths;
use crate::observer::git;
use crate::observer::loc::LocObserver;
use crate::plugin_assets::{self, plugin_dest, ExtractMode, ExtractStats};
use anyhow::{Context, Result};

use crate::observers::{build_calibration, run_all};
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

/// Outcome of the optional Claude-skills install step. The path is the
/// destination directory; the rendering layer composes
/// "<path> (<verb>)" lines from this.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SkillsAction {
    /// Bundled plugin extracted; per-bucket counts pulled from the
    /// extract summary so the user sees what landed.
    Installed {
        added: usize,
        updated: usize,
        unchanged: usize,
    },
    /// User declined the prompt.
    Declined,
    /// `--no-skills` was passed.
    SuppressedByFlag,
    /// `claude` not on `PATH` — silently skipped (no prompt).
    SkippedNoClaude,
    /// stdin is not a TTY and `--yes` wasn't passed — skipped with a
    /// hint pointing at `heal skills install`.
    SkippedNonInteractive,
}

pub fn run(project: &Path, force: bool, yes: bool, no_skills: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    paths
        .ensure()
        .with_context(|| format!("creating {}", paths.root().display()))?;

    let primary_language = LocObserver::default().scan(project).primary;
    let config_action = write_config(&paths, force)?;
    let (hook_action, hook_path) = install_post_commit_hook(project, force)?;
    let severity_counts = run_initial_scan(project, &paths)?;
    let plugin_dest = plugin_dest(project);
    let skills_action = handle_skills_install(project, &plugin_dest, force, yes, no_skills)?;

    print_summary(
        &paths,
        primary_language.as_deref(),
        config_action,
        hook_action,
        hook_path.as_deref(),
        &plugin_dest,
        &skills_action,
        severity_counts.as_ref(),
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn print_summary(
    paths: &HealPaths,
    primary_language: Option<&str>,
    config_action: ConfigAction,
    hook_action: HookAction,
    hook_path: Option<&Path>,
    plugin_dest: &Path,
    skills_action: &SkillsAction,
    severity_counts: Option<&SeverityCounts>,
) {
    println!("HEAL initialized at {}", paths.root().display());
    println!(
        "  primary language: {}",
        primary_language.unwrap_or("(not detected)"),
    );

    println!();
    println!("Installed:");
    println!(
        "  config            {}  ({config_action})",
        paths.config().display(),
    );
    println!("  calibration       {}", paths.calibration().display());
    println!("  initial snapshot  {}/", paths.snapshots_dir().display());
    match hook_path {
        Some(p) => println!("  post-commit hook  {}  ({hook_action})", p.display()),
        None => println!("  post-commit hook  {hook_action}"),
    }
    println!(
        "  Claude plugin     {}",
        render_skills_line(plugin_dest, skills_action),
    );

    if let Some(counts) = severity_counts {
        let colorize = std::io::stdout().is_terminal();
        println!();
        println!("Findings: {}", counts.render_inline(colorize));
        if counts.critical > 0 {
            println!("  → goal: bring [critical] to 0 (try `heal check --severity critical`)");
        }
    }

    println!();
    println!("Next steps:");
    println!("  heal check               # render the Severity-grouped TODO list");
    println!("  heal status              # see metric trends");
    if matches!(
        skills_action,
        SkillsAction::Installed { .. } | SkillsAction::SkippedNoClaude
    ) {
        // No further skills hint for "Installed" (already done) or
        // "SkippedNoClaude" (Claude isn't there to use them anyway).
    } else {
        println!("  heal skills install      # extract the Claude plugin when ready");
    }
}

fn render_skills_line(dest: &Path, action: &SkillsAction) -> String {
    match action {
        SkillsAction::Installed {
            added,
            updated,
            unchanged,
        } => {
            let mut parts = vec![format!("{added} new")];
            if *updated > 0 {
                parts.push(format!("{updated} updated"));
            }
            parts.push(format!("{unchanged} unchanged"));
            format!("{}/  (extracted: {})", dest.display(), parts.join(", "))
        }
        SkillsAction::Declined => "skipped (declined)".to_string(),
        SkillsAction::SuppressedByFlag => "skipped (--no-skills)".to_string(),
        SkillsAction::SkippedNoClaude => "skipped (no `claude` command on PATH)".to_string(),
        SkillsAction::SkippedNonInteractive => {
            "skipped (non-interactive shell; pass `--yes` or run `heal skills install` later)"
                .to_string()
        }
    }
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

fn install_post_commit_hook(project: &Path, force: bool) -> Result<(HookAction, Option<PathBuf>)> {
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

fn run_initial_scan(project: &Path, paths: &HealPaths) -> Result<Option<SeverityCounts>> {
    // Load the just-written (or pre-existing) config so observers honor
    // the project's enable flags. A config-missing error here would
    // indicate a write_config bug — propagate it rather than silently
    // falling back to defaults.
    let cfg = match crate::core::config::load_from_project(project) {
        Ok(c) => c,
        Err(crate::core::Error::ConfigMissing(_)) => Config::default(),
        Err(e) => return Err(e.into()),
    };

    let reports = run_all(project, &cfg, None);

    // Save calibration before packing the snapshot so the freshly
    // saved file is what `classify_with_calibration` reads back.
    let calibration = build_calibration(&reports, &cfg);
    calibration.save(&paths.calibration())?;

    let (_, findings) = snapshot::classify_with_calibration(paths, &cfg, &reports);
    let snap = snapshot::pack(project, paths, &cfg, &reports, &findings);
    let counts = snap.severity_counts;
    let payload = serde_json::to_value(&snap).expect("MetricsSnapshot serialization is infallible");
    EventLog::new(paths.snapshots_dir()).append(&Event::new("init", payload))?;
    Ok(counts)
}

/// Decide whether to install the Claude plugin and do it. Returns the
/// outcome label so the summary block can render "<path> (verb)".
///
/// Decision tree (first match wins):
///   1. `--no-skills` → `SuppressedByFlag`.
///   2. `claude` not on `PATH` → `SkippedNoClaude` (no prompt — the
///      skills are useless without Claude Code anyway).
///   3. `--yes` → install.
///   4. stdin is a TTY → prompt the user (default `Y`).
///   5. otherwise → `SkippedNonInteractive` (with a hint in the
///      summary).
///
/// `force` matches `heal init --force` semantics: when on, refresh the
/// plugin tree (overwriting drift / locally edited files) so a binary
/// upgrade actually picks up the latest skill set. When off, leave
/// existing files alone (initial-install behaviour).
fn handle_skills_install(
    project: &Path,
    dest: &Path,
    force: bool,
    yes: bool,
    no_skills: bool,
) -> Result<SkillsAction> {
    if no_skills {
        return Ok(SkillsAction::SuppressedByFlag);
    }
    if !claude_on_path() {
        return Ok(SkillsAction::SkippedNoClaude);
    }
    if yes {
        return install_skills(project, dest, force);
    }
    if std::io::stdin().is_terminal() {
        if confirm_skills_install()? {
            install_skills(project, dest, force)
        } else {
            Ok(SkillsAction::Declined)
        }
    } else {
        Ok(SkillsAction::SkippedNonInteractive)
    }
}

fn install_skills(project: &Path, dest: &Path, force: bool) -> Result<SkillsAction> {
    let mode = if force {
        // `Update` keeps the manifest in sync; `InstallForce` is reserved for `heal skills install --force`.
        ExtractMode::Update { force: true }
    } else {
        ExtractMode::InstallSafe
    };
    let (stats, manifest) = plugin_assets::extract(dest, mode)?;
    claude_settings::wire(project, &manifest.heal_version)?;
    Ok(extract_counts(&stats))
}

fn extract_counts(stats: &ExtractStats) -> SkillsAction {
    let s = stats.summary();
    SkillsAction::Installed {
        added: s.added,
        updated: s.updated,
        unchanged: s.unchanged + s.skipped,
    }
}

/// Walk `PATH` looking for an executable named `claude`. Pure stdlib so
/// no extra dependency. Heal is Unix-only today so the Windows
/// extension dance is omitted.
fn claude_on_path() -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| dir.join("claude").is_file())
}

fn confirm_skills_install() -> Result<bool> {
    print!(
        "Install the bundled Claude plugin (provides /heal-code-review + /heal-code-patch)? [Y/n] ",
    );
    std::io::stdout()
        .flush()
        .context("flushing skills-install prompt")?;

    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .context("reading skills-install prompt response")?;
    let answer = line.trim().to_ascii_lowercase();
    Ok(matches!(answer.as_str(), "" | "y" | "yes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{commit, init_repo};
    use tempfile::TempDir;

    fn commit_default(cwd: &Path, file: &str, body: &str, email: &str) {
        commit(cwd, file, body, email, "snap");
    }

    fn hook_path_for(project: &Path) -> std::path::PathBuf {
        git::hooks_dir(project)
            .expect("test repo must be initialized before requesting hook path")
            .join("post-commit")
    }

    /// Default invocation for the end-to-end tests: `--no-skills` so the
    /// suite never depends on whether `claude` happens to be on the
    /// runner's PATH.
    fn run_no_skills(project: &Path, force: bool) -> Result<()> {
        run(project, force, false, true)
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
        let (action, path) = install_post_commit_hook(dir.path(), false).unwrap();
        assert_eq!(action, HookAction::SkippedNoRepo);
        assert!(path.is_none(), "hook path is meaningless without a repo");
    }

    #[cfg(unix)]
    #[test]
    fn install_hook_writes_executable_post_commit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let (action, path) = install_post_commit_hook(dir.path(), false).unwrap();
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
    fn install_hook_refreshes_own_marker() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let hook = hook_path_for(dir.path());
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(
            &hook,
            format!("#!/bin/sh\n{HEAL_HOOK_MARKER}\necho stale\n"),
        )
        .unwrap();
        let (action, path) = install_post_commit_hook(dir.path(), false).unwrap();
        assert_eq!(action, HookAction::Refreshed);
        assert_eq!(path.as_deref(), Some(hook.as_path()));
        let body = std::fs::read_to_string(&hook).unwrap();
        assert!(body.contains("heal hook commit"));
        assert!(!body.contains("stale"));
    }

    #[test]
    fn install_hook_preserves_user_hook_without_force() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let hook = hook_path_for(dir.path());
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(&hook, "#!/bin/sh\necho user hook\n").unwrap();
        let (action, _) = install_post_commit_hook(dir.path(), false).unwrap();
        assert_eq!(action, HookAction::SkippedUserHook);
        let body = std::fs::read_to_string(&hook).unwrap();
        assert!(body.contains("echo user hook"));
    }

    #[test]
    fn install_hook_overwrites_user_hook_with_force() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let hook = hook_path_for(dir.path());
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(&hook, "#!/bin/sh\necho user hook\n").unwrap();
        let (action, _) = install_post_commit_hook(dir.path(), true).unwrap();
        assert_eq!(action, HookAction::Overwrote);
        let body = std::fs::read_to_string(&hook).unwrap();
        assert!(body.contains(HEAL_HOOK_MARKER));
    }

    #[test]
    fn run_end_to_end_creates_layout_config_and_snapshot() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_default(dir.path(), "main.rs", "fn main() {}\n", "solo@example.com");
        run_no_skills(dir.path(), false).unwrap();
        let paths = HealPaths::new(dir.path());
        assert!(paths.config().exists(), "config.toml must exist");
        assert!(paths.calibration().exists(), "calibration.toml must exist");
        assert!(paths.snapshots_dir().exists(), "snapshots dir must exist");
        let any_snapshot = std::fs::read_dir(paths.snapshots_dir())
            .unwrap()
            .any(|e| e.is_ok());
        assert!(any_snapshot, "snapshots dir must contain the init record");
        assert!(
            hook_path_for(dir.path()).exists(),
            "post-commit hook must be installed",
        );

        let log = crate::core::eventlog::EventLog::new(paths.snapshots_dir());
        let (_, metrics) = crate::core::snapshot::MetricsSnapshot::latest_in(&log)
            .unwrap()
            .expect("init must write a snapshot record");
        assert!(
            metrics.severity_counts.is_some(),
            "snapshot must carry severity_counts after pack loads calibration.toml"
        );
        assert!(
            metrics.codebase_files.is_some(),
            "snapshot must carry codebase_files for the recalibrate trigger"
        );
    }

    #[test]
    fn no_skills_flag_leaves_plugin_dir_unwritten() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_default(dir.path(), "main.rs", "fn main() {}\n", "solo@example.com");
        run_no_skills(dir.path(), false).unwrap();
        assert!(
            !plugin_dest(dir.path()).exists(),
            "--no-skills must not extract the plugin"
        );
    }

    #[test]
    fn handle_skills_install_respects_no_skills_flag() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let dest = plugin_dest(project);
        let action = handle_skills_install(project, &dest, false, false, true).unwrap();
        assert_eq!(action, SkillsAction::SuppressedByFlag);
        assert!(!dest.exists());
    }

    #[test]
    fn handle_skills_install_with_yes_extracts_plugin_when_claude_available() {
        // Stage a fake `claude` binary on PATH so the prompt logic
        // believes Claude Code is installed. Without this, the call
        // legitimately returns SkippedNoClaude on hosts that don't
        // happen to have `claude` on PATH.
        let bin_dir = TempDir::new().unwrap();
        let claude_bin = bin_dir.path().join("claude");
        std::fs::write(&claude_bin, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&claude_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let original_path = std::env::var_os("PATH").unwrap_or_default();
        let mut new_path = std::ffi::OsString::from(bin_dir.path());
        new_path.push(":");
        new_path.push(&original_path);
        let _guard = PathGuard::set(new_path);

        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let dest = plugin_dest(project);
        let action = handle_skills_install(project, &dest, false, true, false).unwrap();
        assert!(matches!(action, SkillsAction::Installed { .. }));
        assert!(dest.exists(), "yes path must extract the plugin");
        assert!(dest.join("plugin.json").exists());
        assert!(
            project.join(".claude-plugin/marketplace.json").exists(),
            "init must wire the local marketplace alongside the plugin tree"
        );
        assert!(
            project.join(".claude/settings.json").exists(),
            "init must register the marketplace in settings.json"
        );
    }

    #[test]
    fn handle_skills_install_skips_when_no_claude() {
        // Pretend PATH is empty so the claude lookup fails
        // deterministically regardless of host environment.
        let _guard = PathGuard::set(std::ffi::OsString::new());
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let dest = plugin_dest(project);
        let action = handle_skills_install(project, &dest, false, true, false).unwrap();
        assert_eq!(action, SkillsAction::SkippedNoClaude);
        assert!(!dest.exists());
    }

    #[test]
    fn install_skills_force_overwrites_drifted_files() {
        // First install: clean extraction.
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let dest = project.join("plugin");
        let initial = install_skills(project, &dest, false).unwrap();
        let SkillsAction::Installed {
            added: initial_added,
            updated: initial_updated,
            ..
        } = initial
        else {
            panic!("expected Installed, got {initial:?}");
        };
        assert!(initial_added > 0);
        assert_eq!(initial_updated, 0, "no drift on first install");

        // Tamper with a known-shipped skill file.
        let skill = dest.join("skills/heal-code-patch/SKILL.md");
        assert!(skill.exists(), "fixture should have shipped this skill");
        std::fs::write(&skill, "tampered\n").unwrap();

        // Refresh path: force=true should overwrite even drifted files.
        let refreshed = install_skills(project, &dest, true).unwrap();
        let SkillsAction::Installed {
            updated: refreshed_updated,
            ..
        } = refreshed
        else {
            panic!("expected Installed, got {refreshed:?}");
        };
        assert!(
            refreshed_updated > 0,
            "force refresh must report updated files"
        );
        assert_ne!(
            std::fs::read_to_string(&skill).unwrap(),
            "tampered\n",
            "force refresh must overwrite drifted skill content"
        );
    }

    #[test]
    fn install_skills_no_force_preserves_existing_files() {
        // First install seeds the manifest.
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let dest = project.join("plugin");
        install_skills(project, &dest, false).unwrap();

        // Tamper with a skill — without --force we expect it preserved.
        let skill = dest.join("skills/heal-code-patch/SKILL.md");
        std::fs::write(&skill, "tampered\n").unwrap();

        let action = install_skills(project, &dest, false).unwrap();
        let SkillsAction::Installed { updated, .. } = action else {
            panic!("expected Installed, got {action:?}");
        };
        assert_eq!(updated, 0, "InstallSafe must not overwrite anything");
        assert_eq!(
            std::fs::read_to_string(&skill).unwrap(),
            "tampered\n",
            "non-force install must leave the user-edited file alone"
        );
    }

    /// RAII guard so individual tests can mutate `PATH` without leaking
    /// the change into siblings. The static `Mutex` serializes
    /// PathGuard-holding tests so concurrent ones don't trample each
    /// other's expected `claude_on_path()` outcome; `test_support::git`
    /// caches the git binary path so non-PathGuard tests that shell out
    /// to git don't observe transient `PATH=""` either.
    struct PathGuard {
        original: Option<std::ffi::OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    impl PathGuard {
        fn set(value: std::ffi::OsString) -> Self {
            let lock = PATH_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let original = std::env::var_os("PATH");
            std::env::set_var("PATH", value);
            Self {
                original,
                _lock: lock,
            }
        }
    }

    impl Drop for PathGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(v) => std::env::set_var("PATH", v),
                None => std::env::remove_var("PATH"),
            }
        }
    }
}
