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
//!      the resulting distribution. The fresh calibration captures
//!      `meta.calibrated_at_sha` / `meta.codebase_files` so the
//!      `heal-setup` skill can later judge drift without consulting any
//!      event log.
//!   6. Optionally extract the bundled skills, once per detected agent
//!      target — `.claude/skills/` for Claude Code, `.agents/skills/`
//!      for Codex CLI. The Claude path also sweeps legacy hook entries
//!      from `.claude/settings.json`. Each target is decided
//!      independently (prompted per-target when stdin is a TTY; bypassed
//!      with `--yes` / `--no-skills`).

use std::fmt;
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

use crate::claude_settings;
use crate::commands::hook_install::{self, HookAction};
use crate::core::config::Config;
use crate::core::monorepo::{self, MonorepoSignal};
use crate::core::severity::SeverityCounts;
use crate::core::HealPaths;
use crate::skill_assets::{self, agent_on_path, ExtractMode, ExtractStats, SkillTarget};
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

/// Per-target outcome of the optional skills install step. Doubles as
/// the JSON shape under `init --json`'s `skills[].action` discriminator
/// — the variant tag becomes `action: "<snake_case>"` and the
/// `Installed` variant's fields flatten in alongside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum SkillsAction {
    Installed {
        added: usize,
        updated: usize,
        unchanged: usize,
    },
    Declined,
    SuppressedByFlag,
    /// The agent's CLI is not on `PATH`, so the skills would have
    /// nowhere to be invoked from. `agent` is the executable name we
    /// looked for (e.g. `"claude"`, `"codex"`).
    SkippedNotInstalled {
        agent: &'static str,
    },
    SkippedNonInteractive,
}

#[allow(clippy::fn_params_excessive_bools)] // each flag is independent CLI surface
pub fn run(
    project: &Path,
    force: bool,
    yes: bool,
    no_skills: bool,
    as_json: bool,
    explicit: bool,
) -> Result<()> {
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
    } = run_initial_scan(project, &paths)?;
    let skills_outcomes = handle_skills_install(project, &paths, force, yes, no_skills)?;
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
            &hook_action,
            hook_path.as_deref(),
            &skills_outcomes,
            severity_counts.as_ref(),
            &monorepo_signals,
        ));
        return Ok(());
    }

    print_summary(
        &paths,
        primary_language.as_deref(),
        config_action,
        hook_action,
        hook_path.as_deref(),
        &skills_outcomes,
        severity_counts.as_ref(),
        &monorepo_signals,
    );
    Ok(())
}

/// Stable JSON contract for `heal init --json`. Mirrors the lines the
/// human renderer emits but in a typed shape so scripts and the
/// `heal-setup` skill can act on it without parsing free-form text.
#[derive(Debug, Serialize)]
struct InitReport<'a> {
    project: String,
    heal_dir: String,
    primary_language: Option<&'a str>,
    config: PathAction<'a, ConfigAction>,
    calibration_path: String,
    post_commit_hook: PathAction<'a, HookAction>,
    /// One report per agent target HEAL knows about (Claude, Codex,
    /// …), in [`SkillTarget::ALL`] order. Each entry self-describes
    /// the destination path and outcome — even targets whose CLI is
    /// not installed appear with a `skipped_not_installed` action so
    /// downstream tooling can see what was considered.
    skills: Vec<SkillsTargetReport<'a>>,
    severity_counts: Option<&'a SeverityCounts>,
    /// Manifests detected in the project root that suggest a monorepo
    /// layout the user may want to declare via `[[project.workspaces]]`.
    /// Empty when no signals fire OR when workspaces are already
    /// declared — the `heal-setup` skill keys off this to decide
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

/// One entry in [`InitReport::skills`] — the install verdict for a
/// single agent target.
#[derive(Debug, Serialize)]
struct SkillsTargetReport<'a> {
    target: SkillTarget,
    dest: String,
    #[serde(flatten)]
    action: &'a SkillsAction,
}

/// Internal pairing of `(target, action)` produced by
/// [`handle_skills_install`]. Owned so the renderer and the JSON
/// encoder can both borrow from it without an extra allocation.
#[derive(Debug)]
struct SkillsTargetOutcome {
    target: SkillTarget,
    action: SkillsAction,
}

impl<'a> InitReport<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        project: &Path,
        paths: &HealPaths,
        primary_language: Option<&'a str>,
        config_action: &'a ConfigAction,
        hook_action: &'a HookAction,
        hook_path: Option<&Path>,
        skills_outcomes: &'a [SkillsTargetOutcome],
        severity_counts: Option<&'a SeverityCounts>,
        monorepo_signals: &'a [MonorepoSignal],
    ) -> Self {
        let skills = skills_outcomes
            .iter()
            .map(|o| SkillsTargetReport {
                target: o.target,
                dest: o.target.dest(project).display().to_string(),
                action: &o.action,
            })
            .collect();
        Self {
            project: project.display().to_string(),
            heal_dir: paths.root().display().to_string(),
            primary_language,
            config: PathAction {
                path: Some(paths.config().display().to_string()),
                action: config_action,
            },
            calibration_path: paths.calibration().display().to_string(),
            post_commit_hook: PathAction {
                path: hook_path.map(|p| p.display().to_string()),
                action: hook_action,
            },
            skills,
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
    hook_action: HookAction,
    hook_path: Option<&Path>,
    skills_outcomes: &[SkillsTargetOutcome],
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
    println!("  calibration       {}", paths.calibration().display());
    match hook_path {
        Some(p) => println!("  post-commit hook  {}  ({hook_action})", p.display()),
        None => println!("  post-commit hook  {hook_action}"),
    }
    for outcome in skills_outcomes {
        println!(
            "  {} skills{} {}",
            outcome.target.display_name(),
            // pad shorter labels so the dest column lines up
            " ".repeat(skills_label_padding(outcome.target)),
            render_skills_line(outcome.target, &outcome.action),
        );
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
             scopes per package — run `/heal-setup` in any installed agent to set this up.",
        );
    }

    println!();
    println!("Next steps:");
    println!("  heal status               # render the Severity-grouped TODO list");
    println!("  heal metrics              # see metric trends");
    println!("  heal diff                 # progress vs. the calibration baseline");
    let any_installed = skills_outcomes
        .iter()
        .any(|o| matches!(o.action, SkillsAction::Installed { .. }));
    let any_skip_for_install = skills_outcomes.iter().any(|o| {
        matches!(
            o.action,
            SkillsAction::Declined
                | SkillsAction::SuppressedByFlag
                | SkillsAction::SkippedNonInteractive
        )
    });
    if any_installed {
        println!();
        println!("Skills (run from any installed agent):");
        println!("  /heal-setup        # tune thresholds, enable optional features");
        println!("  /heal-code-review  # architectural reading + refactor TODO");
        println!("  /heal-code-patch   # drain the cache, one fix per commit");
    } else if any_skip_for_install {
        println!("  heal skills install       # extract the bundled skills when ready");
    }
}

/// Pre-computed widest `display_name()` length across every
/// [`SkillTarget`] variant. The renderer pads each agent's left
/// label up to this width so the dest column lines up.
const WIDEST_DISPLAY_NAME: usize = {
    let mut max = 0;
    let mut i = 0;
    while i < SkillTarget::ALL.len() {
        let len = SkillTarget::ALL[i].display_name().len();
        if len > max {
            max = len;
        }
        i += 1;
    }
    max
};

fn skills_label_padding(target: SkillTarget) -> usize {
    WIDEST_DISPLAY_NAME.saturating_sub(target.display_name().len())
}

fn render_skills_line(target: SkillTarget, action: &SkillsAction) -> String {
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
            format!("{}/  (extracted: {})", target.dest_rel(), parts.join(", "))
        }
        SkillsAction::Declined => "skipped (declined)".to_string(),
        SkillsAction::SuppressedByFlag => "skipped (--no-skills)".to_string(),
        SkillsAction::SkippedNotInstalled { agent } => {
            format!("skipped (no `{agent}` command on PATH)")
        }
        SkillsAction::SkippedNonInteractive => {
            "skipped (non-interactive shell; pass `--yes` or run `heal skills install` later)"
                .to_string()
        }
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
}

fn run_initial_scan(project: &Path, paths: &HealPaths) -> Result<InitialScan> {
    // Load the just-written (or pre-existing) config so observers honor
    // the project's enable flags. A config-missing error here would
    // indicate a write_config bug — propagate it rather than silently
    // falling back to defaults.
    let cfg = match crate::core::config::load_from_project(project) {
        Ok(c) => c,
        Err(crate::core::Error::ConfigMissing(_)) => Config::default(),
        Err(e) => return Err(e.into()),
    };

    let reports = run_all(project, &cfg, None, None);
    let primary_language = reports.loc.primary.clone();
    let calibration = build_calibration(project, &reports, &cfg);
    calibration.save(&paths.calibration())?;

    let cal_with_overrides = calibration.with_overrides(&cfg);
    let findings = classify(&reports, &cal_with_overrides, &cfg);
    Ok(InitialScan {
        cfg,
        primary_language,
        severity_counts: Some(SeverityCounts::from_findings(&findings)),
    })
}

/// Decide per agent target whether to install the bundled skills and
/// do it. Returns one outcome per [`SkillTarget`] in
/// [`SkillTarget::ALL`] order so the summary block can render every
/// considered target — even ones whose CLI is absent.
///
/// Per-target decision tree (first match wins):
///   1. `--no-skills` → `SuppressedByFlag` for every target.
///   2. The target's CLI is not on `PATH` → `SkippedNotInstalled` (no
///      prompt — the skills are useless without that agent anyway).
///   3. `--yes` → install.
///   4. stdin is a TTY → prompt the user (default `Y`), once per
///      detected target so users can opt into one agent and skip the
///      other.
///   5. otherwise → `SkippedNonInteractive`.
///
/// `force` matches `heal init --force` semantics: when on, refresh the
/// skills tree (overwriting drift / locally edited files) so a binary
/// upgrade actually picks up the latest skill set. When off, leave
/// existing files alone (initial-install behavior).
fn handle_skills_install(
    project: &Path,
    paths: &HealPaths,
    force: bool,
    yes: bool,
    no_skills: bool,
) -> Result<Vec<SkillsTargetOutcome>> {
    // Snapshot detection up-front so the per-target loop sees a stable
    // view; also lets tests stub the detection without mutating
    // process-wide `PATH` (which races with parallel test workers that
    // shell out to `git`).
    let detected: Vec<(SkillTarget, bool)> = SkillTarget::ALL
        .iter()
        .map(|&t| (t, agent_on_path(t)))
        .collect();
    handle_skills_install_with(project, paths, force, yes, no_skills, &detected)
}

fn handle_skills_install_with(
    project: &Path,
    paths: &HealPaths,
    force: bool,
    yes: bool,
    no_skills: bool,
    detected: &[(SkillTarget, bool)],
) -> Result<Vec<SkillsTargetOutcome>> {
    let mut outcomes = Vec::with_capacity(detected.len());
    for &(target, on_path) in detected {
        let action = decide_target(project, paths, target, force, yes, no_skills, on_path)?;
        outcomes.push(SkillsTargetOutcome { target, action });
    }
    Ok(outcomes)
}

#[allow(clippy::fn_params_excessive_bools)]
fn decide_target(
    project: &Path,
    paths: &HealPaths,
    target: SkillTarget,
    force: bool,
    yes: bool,
    no_skills: bool,
    on_path: bool,
) -> Result<SkillsAction> {
    if no_skills {
        return Ok(SkillsAction::SuppressedByFlag);
    }
    if !on_path {
        return Ok(SkillsAction::SkippedNotInstalled {
            agent: target.cli_name(),
        });
    }
    if yes {
        return install_skills_for(project, paths, target, force);
    }
    if std::io::stdin().is_terminal() {
        if confirm_skills_install(target)? {
            install_skills_for(project, paths, target, force)
        } else {
            Ok(SkillsAction::Declined)
        }
    } else {
        Ok(SkillsAction::SkippedNonInteractive)
    }
}

fn install_skills_for(
    project: &Path,
    _paths: &HealPaths,
    target: SkillTarget,
    force: bool,
) -> Result<SkillsAction> {
    let mode = if force {
        // `Update { force }` overwrites every file regardless of drift,
        // matching the "refresh on heal init --force" semantics.
        ExtractMode::Update { force: true }
    } else {
        ExtractMode::InstallSafe
    };
    let dest = target.dest(project);
    let stats = skill_assets::extract(&dest, mode)?;
    // Only the Claude path has a settings.json to sweep. Codex relies
    // on plain skill discovery under `.agents/skills/` — there's no
    // sibling settings file to maintain.
    if matches!(target, SkillTarget::Claude) {
        claude_settings::wire(project)?;
    }
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

fn confirm_skills_install(target: SkillTarget) -> Result<bool> {
    print!(
        "Install the bundled HEAL skills for {} (under `{}/`)? [Y/n] ",
        target.display_name(),
        target.dest_rel(),
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

    /// Default invocation for the end-to-end tests: `--no-skills` so the
    /// suite never depends on whether `claude` happens to be on the
    /// runner's PATH.
    fn run_no_skills(project: &Path, force: bool) -> Result<()> {
        run(project, force, false, true, false, false)
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
        run_no_skills(dir.path(), false).unwrap();
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
    fn no_skills_flag_leaves_skills_dir_unwritten() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        commit_default(dir.path(), "main.rs", "fn main() {}\n", "solo@example.com");
        run_no_skills(dir.path(), false).unwrap();
        for &target in &SkillTarget::ALL {
            assert!(
                !target.dest(dir.path()).exists(),
                "--no-skills must not extract the skill set for {target:?}",
            );
        }
    }

    fn detected(claude: bool, codex: bool) -> [(SkillTarget, bool); 2] {
        [(SkillTarget::Claude, claude), (SkillTarget::Codex, codex)]
    }

    #[test]
    fn handle_skills_install_respects_no_skills_flag() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        // `--no-skills` must short-circuit before detection matters.
        let outcomes =
            handle_skills_install_with(project, &paths, false, false, true, &detected(true, true))
                .unwrap();
        assert_eq!(outcomes.len(), SkillTarget::ALL.len());
        for outcome in &outcomes {
            assert_eq!(outcome.action, SkillsAction::SuppressedByFlag);
            assert!(!outcome.target.dest(project).exists());
        }
    }

    #[test]
    fn handle_skills_install_with_yes_extracts_for_each_detected_agent() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        let outcomes =
            handle_skills_install_with(project, &paths, false, true, false, &detected(true, true))
                .unwrap();
        assert_eq!(outcomes.len(), SkillTarget::ALL.len());
        for outcome in &outcomes {
            assert!(
                matches!(outcome.action, SkillsAction::Installed { .. }),
                "expected Installed for {target:?}, got {action:?}",
                target = outcome.target,
                action = outcome.action,
            );
            let dest = outcome.target.dest(project);
            assert!(
                dest.exists(),
                "{target:?} dest must exist",
                target = outcome.target
            );
            assert!(dest.join("heal-cli/SKILL.md").exists());
        }
    }

    #[test]
    fn handle_skills_install_with_only_codex_skips_claude_target() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        let outcomes =
            handle_skills_install_with(project, &paths, false, true, false, &detected(false, true))
                .unwrap();

        let claude = outcomes
            .iter()
            .find(|o| o.target == SkillTarget::Claude)
            .unwrap();
        assert_eq!(
            claude.action,
            SkillsAction::SkippedNotInstalled { agent: "claude" },
        );
        assert!(!SkillTarget::Claude.dest(project).exists());

        let codex = outcomes
            .iter()
            .find(|o| o.target == SkillTarget::Codex)
            .unwrap();
        assert!(matches!(codex.action, SkillsAction::Installed { .. }));
        assert!(SkillTarget::Codex
            .dest(project)
            .join("heal-cli/SKILL.md")
            .exists());
    }

    #[test]
    fn handle_skills_install_skips_every_target_when_no_agent_present() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        let outcomes = handle_skills_install_with(
            project,
            &paths,
            false,
            true,
            false,
            &detected(false, false),
        )
        .unwrap();
        assert_eq!(outcomes.len(), SkillTarget::ALL.len());
        for outcome in &outcomes {
            assert!(
                matches!(outcome.action, SkillsAction::SkippedNotInstalled { .. }),
                "expected SkippedNotInstalled for {target:?}, got {action:?}",
                target = outcome.target,
                action = outcome.action,
            );
            assert!(!outcome.target.dest(project).exists());
        }
    }

    // No PATH-mutating test here intentionally: cargo's parallel
    // scheduler races such tests against any sibling that shells out
    // to git (commits, in particular), because git's child processes
    // resolve `PATH` at execve time. `handle_skills_install_with`
    // takes detection as input precisely so we can stub it without
    // touching process-wide state. `agent_on_path` itself is a thin
    // `split_paths` loop — covered indirectly via `from_path` on hosts
    // that have or lack each agent, no dedicated test needed.

    #[test]
    fn install_skills_for_force_overwrites_drifted_files() {
        // First install: clean extraction into the Claude target.
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        let target = SkillTarget::Claude;
        let initial = install_skills_for(project, &paths, target, false).unwrap();
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
        let dest = target.dest(project);
        let skill = dest.join("heal-code-patch/SKILL.md");
        assert!(skill.exists(), "fixture should have shipped this skill");
        std::fs::write(&skill, "tampered\n").unwrap();

        // Refresh path: force=true should overwrite even drifted files.
        let refreshed = install_skills_for(project, &paths, target, true).unwrap();
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
    fn install_skills_for_no_force_preserves_existing_files() {
        // First install seeds the on-disk metadata stamp.
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        let target = SkillTarget::Claude;
        install_skills_for(project, &paths, target, false).unwrap();

        // Tamper with a skill — without --force we expect it preserved.
        let dest = target.dest(project);
        let skill = dest.join("heal-code-patch/SKILL.md");
        std::fs::write(&skill, "tampered\n").unwrap();

        let action = install_skills_for(project, &paths, target, false).unwrap();
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

    #[test]
    fn install_skills_for_codex_does_not_touch_claude_settings() {
        // Codex install must not write `.claude/settings.json` —
        // settings wiring is Claude-specific.
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        install_skills_for(project, &paths, SkillTarget::Codex, false).unwrap();
        assert!(SkillTarget::Codex
            .dest(project)
            .join("heal-cli/SKILL.md")
            .exists());
        assert!(
            !project.join(".claude/settings.json").exists(),
            "codex install must not create .claude/settings.json",
        );
        assert!(
            !SkillTarget::Claude.dest(project).exists(),
            "codex install must not write to .claude/skills/",
        );
    }
}
