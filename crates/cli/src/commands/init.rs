//! `heal init` — wire HEAL into a project.
//!
//! Steps, in order:
//!   1. Ensure `.heal/` layout exists.
//!   2. Detect the primary language via `LocObserver` for the user-facing
//!      summary (not persisted — `heal metrics` re-detects on every call).
//!   3. Write a default `config.toml` (skipped when one already exists
//!      unless `--force`).
//!   4. Install a `post-commit` git hook that calls `heal hook commit`.
//!   5. Run an initial scan and derive `.heal/calibration.toml` from
//!      the resulting distribution, unless one already exists and
//!      `--force` is off. The calibration captures
//!      `meta.calibrated_at_sha` / `meta.codebase_files` so
//!      `heal doctor` can later judge drift without any event log.
//!
//! Skills are not installed here: they ship as the heal Claude Code
//! plugin. The summary points at the plugin, and at
//! `heal skills uninstall` when an older install left skill folders in
//! the project.

use std::fmt;
use std::io::IsTerminal;
use std::path::Path;

use crate::commands::hook_install::{self, HookAction};
use crate::core::calibration::Calibration;
use crate::core::config::Config;
use crate::core::monorepo::{self, MonorepoSignal};
use crate::core::severity::SeverityCounts;
use crate::core::HealPaths;
use crate::legacy_skills;
use anyhow::{Context, Result};
use serde::Serialize;

use crate::observers::{build_calibration, classify, run_all};

/// Outcome of writing the project's `config.toml`. The `tag = "action"`
/// attribute makes this safe to `#[serde(flatten)]` next to a `path:`
/// sibling — unit variants serialize as `{ "action": "wrote" }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
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

#[allow(clippy::fn_params_excessive_bools)] // each flag is independent CLI surface
pub fn run(project: &Path, force: bool, as_json: bool, explicit: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    paths
        .ensure()
        .with_context(|| format!("creating {}", paths.root().display()))?;

    let config_action = write_config(&paths, force, explicit)?;
    let (hook_action, hook_path) = hook_install::install(project, force)?;
    let InitialScan {
        cfg,
        primary_language,
        severity_counts,
        calibration_action,
    } = run_initial_scan(project, &paths, force)?;
    let legacy_skill_dirs: Vec<String> = legacy_skills::find(project)
        .iter()
        .map(|p| p.strip_prefix(project).unwrap_or(p).display().to_string())
        .collect();
    // Surface workspace manifests only when no `[[project.workspaces]]`
    // block exists yet; once the user declares them, the hint becomes
    // noise. Empty list = solo package or already-declared workspaces.
    let monorepo_signals = if cfg.project.workspaces.is_empty() {
        let mut sigs = monorepo::detect(project);
        monorepo::enrich_with_languages(project, &cfg, &mut sigs);
        sigs
    } else {
        Vec::new()
    };

    if as_json {
        super::emit_json(&InitReport::new(
            project,
            &paths,
            primary_language.as_deref(),
            &config_action,
            &calibration_action,
            &hook_action,
            hook_path.as_deref(),
            &legacy_skill_dirs,
            severity_counts.as_ref(),
            &monorepo_signals,
        ));
        return Ok(());
    }

    print_summary(
        &paths,
        primary_language.as_deref(),
        config_action,
        calibration_action,
        hook_action,
        hook_path.as_deref(),
        &legacy_skill_dirs,
        severity_counts.as_ref(),
        &monorepo_signals,
    );
    Ok(())
}

/// Stable JSON contract for `heal init --json`. Mirrors the lines the
/// human renderer emits but in a typed shape so scripts and the
/// `/heal:setup` skill can act on it without parsing free-form text.
#[derive(Debug, Serialize)]
struct InitReport<'a> {
    project: String,
    heal_dir: String,
    primary_language: Option<&'a str>,
    config: PathAction<'a, ConfigAction>,
    calibration_path: String,
    /// Same `{ path, action }` shape as `config`: an existing
    /// `calibration.toml` is kept unless `--force`.
    calibration: PathAction<'a, ConfigAction>,
    post_commit_hook: PathAction<'a, HookAction>,
    /// Skill folders an older heal copied into the project
    /// (project-relative); `heal skills uninstall` removes them.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    legacy_skills: &'a [String],
    severity_counts: Option<&'a SeverityCounts>,
    /// Manifests detected in the project root that suggest a monorepo
    /// layout the user may want to declare via `[[project.workspaces]]`.
    /// Empty when no signals fire OR when workspaces are already
    /// declared — the `/heal:setup` skill keys off this to decide
    /// whether to run its workspace-declaration phase.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    monorepo_signals: &'a [MonorepoSignal],
}

/// Common shape for "we did something to a file" — used twice in
/// `InitReport` (config, `post_commit_hook`). The `path` field is
/// `Option<String>` so the hook entry can omit it when no git repo
/// was present.
#[derive(Debug, Serialize)]
struct PathAction<'a, A: Serialize> {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(flatten)]
    action: &'a A,
}

impl<'a> InitReport<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        project: &Path,
        paths: &HealPaths,
        primary_language: Option<&'a str>,
        config_action: &'a ConfigAction,
        calibration_action: &'a ConfigAction,
        hook_action: &'a HookAction,
        hook_path: Option<&Path>,
        legacy_skills: &'a [String],
        severity_counts: Option<&'a SeverityCounts>,
        monorepo_signals: &'a [MonorepoSignal],
    ) -> Self {
        Self {
            project: project.display().to_string(),
            heal_dir: paths.root().display().to_string(),
            primary_language,
            config: PathAction {
                path: Some(paths.config().display().to_string()),
                action: config_action,
            },
            calibration_path: paths.calibration().display().to_string(),
            calibration: PathAction {
                path: Some(paths.calibration().display().to_string()),
                action: calibration_action,
            },
            post_commit_hook: PathAction {
                path: hook_path.map(|p| p.display().to_string()),
                action: hook_action,
            },
            legacy_skills,
            severity_counts,
            monorepo_signals,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn print_summary(
    paths: &HealPaths,
    primary_language: Option<&str>,
    config_action: ConfigAction,
    calibration_action: ConfigAction,
    hook_action: HookAction,
    hook_path: Option<&Path>,
    legacy_skills: &[String],
    severity_counts: Option<&SeverityCounts>,
    monorepo_signals: &[MonorepoSignal],
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
    println!(
        "  calibration       {}  ({calibration_action})",
        paths.calibration().display(),
    );
    match hook_path {
        Some(p) => println!("  post-commit hook  {}  ({hook_action})", p.display()),
        None => println!("  post-commit hook  {hook_action}"),
    }

    if let Some(counts) = severity_counts {
        let colorize = std::io::stdout().is_terminal();
        println!();
        println!("Findings: {}", counts.render_inline(colorize));
    }

    if !monorepo_signals.is_empty() {
        println!();
        println!("Workspace detected:");
        for s in monorepo_signals {
            println!("  - via {} ({})", s.manifest, s.kind);
            for m in &s.members {
                let lang = m
                    .primary_language
                    .as_deref()
                    .unwrap_or("primary language not detected");
                println!("      {} ({lang})", m.path);
            }
        }
        println!(
            "  → declare workspaces in `[[project.workspaces]]` so calibration\n    \
             scopes per package — run `/heal:setup` in Claude Code to set this up.",
        );
    }

    println!();
    println!("Next steps:");
    println!("  heal status               # render the Tier/Severity/score-ranked TODO list");
    println!("  heal metrics              # see metric trends");
    println!("  heal diff                 # progress vs. the calibration baseline");
    println!();
    println!("Skills ship as a Claude Code plugin. In Claude Code, run:");
    println!("  {}", crate::commands::skills::PLUGIN_INSTALL);
    println!("then /heal:setup to tune thresholds and enable optional features.");
    if !legacy_skills.is_empty() {
        println!();
        println!(
            "{} skill folder(s) from an older heal are in this project; remove them with `heal skills uninstall`.",
            legacy_skills.len()
        );
    }
}

fn write_config(paths: &HealPaths, force: bool, explicit: bool) -> Result<ConfigAction> {
    let cfg_path = paths.config();
    let already_present = cfg_path.exists();
    if already_present && !force {
        return Ok(ConfigAction::KeptExisting);
    }
    let cfg = Config::default();
    if explicit {
        cfg.save_explicit(&cfg_path)?;
    } else {
        cfg.save(&cfg_path)?;
    }
    Ok(if already_present {
        ConfigAction::Overwrote
    } else {
        ConfigAction::Wrote
    })
}

struct InitialScan {
    cfg: Config,
    primary_language: Option<String>,
    severity_counts: Option<SeverityCounts>,
    calibration_action: ConfigAction,
}

/// Scan once for the summary and classify against the calibration.
///
/// An existing `calibration.toml` that loads is kept unless `force`:
/// re-running `heal init` (e.g. to reinstall the hook) must not move the
/// Severity thresholds behind the user's back (`scope.md` R3). A missing
/// or unreadable file is (re)built from this scan.
fn run_initial_scan(project: &Path, paths: &HealPaths, force: bool) -> Result<InitialScan> {
    // Load the just-written (or pre-existing) config so observers honor
    // the project's enable flags. A config-missing error here would
    // indicate a write_config bug — propagate it rather than silently
    // falling back to defaults.
    let cfg = match crate::core::config::load_from_project(project) {
        Ok(c) => c,
        Err(crate::core::Error::ConfigMissing(_)) => Config::default(),
        Err(e) => return Err(e.into()),
    };

    let reports = run_all(project, &cfg, None, None, None);
    let primary_language = reports.loc.primary.clone();
    let calibration_path = paths.calibration();
    let existing = calibration_path
        .exists()
        .then(|| Calibration::load(&calibration_path).ok())
        .flatten();
    let (calibration, calibration_action) = match existing {
        Some(kept) if !force => (kept, ConfigAction::KeptExisting),
        _ => {
            let action = if calibration_path.exists() {
                ConfigAction::Overwrote
            } else {
                ConfigAction::Wrote
            };
            let built = build_calibration(project, &reports, &cfg);
            built.save(&calibration_path)?;
            (built, action)
        }
    };

    let cal_with_overrides = calibration.with_overrides(&cfg);
    let findings = classify(&reports, &cal_with_overrides, &cfg);
    Ok(InitialScan {
        cfg,
        primary_language,
        severity_counts: Some(SeverityCounts::from_findings(&findings)),
        calibration_action,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{commit, init_repo};
    use tempfile::TempDir;

    fn commit_default(cwd: &Path, file: &str, body: &str, email: &str) {
        commit(cwd, file, body, email, "snap");
    }

    /// Default invocation for the end-to-end tests: text output, minimal
    /// config.
    fn run_init(project: &Path, force: bool) -> Result<()> {
        run(project, force, false, false)
    }

    #[test]
    fn write_config_writes_default_when_absent() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        let action = write_config(&paths, false, false).unwrap();
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
        let action = write_config(&paths, false, false).unwrap();
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
        let action = write_config(&paths, true, false).unwrap();
        assert_eq!(action, ConfigAction::Overwrote);
        let cfg = Config::load(&paths.config()).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn write_config_minimal_default_emits_near_empty_body() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        write_config(&paths, false, false).unwrap();
        let body = std::fs::read_to_string(paths.config()).unwrap();
        // The minimal serializer drops every key whose value matches
        // the serde default. `Config::default()` matches verbatim, so
        // none of these stock-default lines should appear.
        for noise in [
            "since_days = 90",
            "top_n = 5",
            "enabled = true",
            "max_loc_threshold = 200000",
            "min_coupling = 3",
            "[features.test.coverage]",
            "[features.docs.standalone]",
            "[policy.drain]",
        ] {
            assert!(
                !body.contains(noise),
                "minimal body should not restate default `{noise}`, got:\n{body}",
            );
        }
        // Round-trip: minimal body is still parseable to the same Config.
        let cfg = Config::load(&paths.config()).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn write_config_explicit_emits_full_default_body() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        write_config(&paths, false, true).unwrap();
        let body = std::fs::read_to_string(paths.config()).unwrap();
        // Spot-check a handful of fields that the minimal form
        // suppresses but the explicit form must restate.
        for surface in [
            "since_days = 90",
            "[metrics]",
            "top_n = 5",
            "[policy.drain]",
        ] {
            assert!(
                body.contains(surface),
                "explicit body should restate default `{surface}`, got:\n{body}",
            );
        }
        let cfg = Config::load(&paths.config()).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn run_end_to_end_creates_layout_config_and_calibration() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_default(dir.path(), "main.rs", "fn main() {}\n", "solo@example.com");
        run_init(dir.path(), false).unwrap();
        let paths = HealPaths::new(dir.path());
        assert!(paths.config().exists(), "config.toml must exist");
        assert!(paths.calibration().exists(), "calibration.toml must exist");
        assert!(
            hook_install::hook_path_for(dir.path()).exists(),
            "post-commit hook must be installed",
        );

        let calibration =
            crate::core::calibration::Calibration::load(&paths.calibration()).unwrap();
        assert!(
            calibration.meta.calibrated_at_sha.is_some(),
            "calibrated_at_sha must be captured from HEAD",
        );
        assert!(
            calibration.meta.codebase_files >= 1,
            "calibration must record codebase_files",
        );
    }

    #[test]
    fn rerun_keeps_calibration_unless_forced() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_default(dir.path(), "main.rs", "fn main() {}\n", "solo@example.com");
        run_init(dir.path(), false).unwrap();
        let paths = HealPaths::new(dir.path());
        let first = std::fs::read(paths.calibration()).unwrap();

        commit_default(dir.path(), "lib.rs", "pub fn f() {}\n", "solo@example.com");
        run_init(dir.path(), false).unwrap();
        assert_eq!(
            std::fs::read(paths.calibration()).unwrap(),
            first,
            "a plain re-run must not recalibrate",
        );

        run_init(dir.path(), true).unwrap();
        assert_ne!(
            std::fs::read(paths.calibration()).unwrap(),
            first,
            "--force rebuilds the calibration",
        );
    }
}
