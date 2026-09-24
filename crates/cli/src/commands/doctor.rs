//! `heal doctor` — report which parts of HEAL's setup are in place.
//!
//! Read-only and offline. It inspects `.heal/`, the post-commit hook, the
//! optional feature families, and the per-user key location, then prints
//! one line per area (or a `DoctorReport` with `--json`). The
//! `/heal:setup` skill reads the JSON to decide which setup steps still
//! need doing, so re-running setup only touches what is missing or stale.
//!
//! Doctor only reports. It never recalibrates (`scope.md` R3), never
//! flips a feature on, and never checks the API key against the network
//! (`scope.md` R5) — `heal auth jev status` does that. Exit code is 0
//! whenever the report could be produced, whatever it says.

use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::commands::hook_install::HEAL_HOOK_MARKER;
use crate::core::calibration::Calibration;
use crate::core::concepts::Concepts;
use crate::core::config::{load_from_project, Config};
use crate::core::doc_pairs::DocPairsFile;
use crate::core::findings_cache::{read_fixed, read_latest};
use crate::core::HealPaths;
use crate::legacy_skills;
use crate::observer::code::loc::LocObserver;
use crate::observer::shared::git;
use crate::semantic::credentials::{default_credentials_path, resolve, KeySource};

/// Commits since calibration past which the percentile breaks may no
/// longer describe the codebase.
const STALE_COMMITS: usize = 200;
/// Relative change in the file count since calibration past which the
/// breaks may no longer describe the codebase.
const STALE_FILE_DRIFT: f64 = 0.20;
/// Fixes recorded since calibration that, with no Critical or High left,
/// suggest the codebase has outgrown its thresholds.
const GRADUATED_FIXES: usize = 10;

/// Verdict for one area. `todo` and `error` are the states `/heal:setup`
/// acts on; `warn` needs a human look; `off` is an optional feature that
/// is disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Ok,
    Todo,
    Warn,
    Off,
    Error,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Todo => "todo",
            Self::Warn => "warn",
            Self::Off => "off",
            Self::Error => "error",
        }
    }

    fn needs_action(self) -> bool {
        matches!(self, Self::Todo | Self::Error)
    }
}

/// Fields every area carries: the verdict, one human sentence, and the
/// command or skill that resolves it (absent when nothing is to be done).
#[derive(Debug, Serialize)]
pub(crate) struct Check {
    status: Status,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<String>,
}

impl Check {
    fn new(status: Status, detail: impl Into<String>, fix: Option<&str>) -> Self {
        Self {
            status,
            detail: detail.into(),
            fix: fix.map(str::to_owned),
        }
    }
}

/// Stable JSON contract for `heal doctor --json`.
#[derive(Debug, Serialize)]
pub(crate) struct DoctorReport {
    heal_version: &'static str,
    project: String,
    /// `.heal/config.toml` exists.
    initialized: bool,
    config: ConfigCheck,
    calibration: CalibrationCheck,
    post_commit_hook: Check,
    docs: DocsCheck,
    test: TestCheck,
    semantic: SemanticCheck,
    legacy_skills: LegacySkillsCheck,
    /// Areas whose status is `todo` or `error`, in report order.
    todo: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ConfigCheck {
    #[serde(flatten)]
    check: Check,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_language: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct CalibrationFacts {
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    calibrated_at_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    calibrated_at_files: Option<u32>,
    /// Files the LOC observer sees now — the same count calibration
    /// records, give or take files only the complexity observer parses.
    #[serde(skip_serializing_if = "Option::is_none")]
    current_files: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commits_since: Option<usize>,
    /// `fixed.json` entries recorded after the calibration was built.
    #[serde(skip_serializing_if = "Option::is_none")]
    fixed_since: Option<usize>,
    /// Which drift rules fired: `commits_since_calibration`,
    /// `file_count_drift`, `graduated`. Empty when fresh.
    reasons: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CalibrationCheck {
    #[serde(flatten)]
    check: Check,
    #[serde(flatten)]
    facts: CalibrationFacts,
}

#[derive(Debug, Serialize)]
pub(crate) struct DocsCheck {
    #[serde(flatten)]
    check: Check,
    enabled: bool,
    pairs_path: String,
    /// Pairs in the doc-pairs file; absent when the file is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pairs: Option<usize>,
    /// Paths named in the doc-pairs file that no longer exist.
    missing_paths: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct TestCheck {
    #[serde(flatten)]
    check: Check,
    enabled: bool,
    coverage_enabled: bool,
    /// Entries of `lcov_paths` that exist on disk.
    lcov_found: Vec<String>,
    post_commit_refresh: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct SemanticCheck {
    #[serde(flatten)]
    check: Check,
    enabled: bool,
    /// Concepts in `.heal/concepts.toml`; absent when the file is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    concepts: Option<usize>,
    /// `environment` or `credentials_file`; absent when no key is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    key_source: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct LegacySkillsCheck {
    #[serde(flatten)]
    check: Check,
    /// Project-relative skill directories an older HEAL extracted.
    paths: Vec<String>,
}

pub fn run(project: &Path, as_json: bool) -> Result<()> {
    let report = diagnose(project);
    if as_json {
        super::emit_json(&report);
    } else {
        print_report(&report);
    }
    Ok(())
}

pub(crate) fn diagnose(project: &Path) -> DoctorReport {
    let paths = HealPaths::new(project);
    let initialized = paths.config().exists();
    let (config, cfg) = check_config(project, initialized);
    let calibration = check_calibration(project, &paths, cfg.as_ref());
    let post_commit_hook = check_hook(project, initialized);
    // Feature sections read the loaded config; a missing or broken file
    // falls back to the defaults, where every optional family is off.
    let effective = cfg.clone().unwrap_or_default();
    let docs = check_docs(project, &effective);
    let test = check_test(project, &effective);
    let key_source = key_source();
    let semantic = check_semantic(&paths, &effective, key_source);
    let legacy_skills = check_legacy_skills(project);

    let todo = [
        ("config", config.check.status),
        ("calibration", calibration.check.status),
        ("post_commit_hook", post_commit_hook.status),
        ("docs", docs.check.status),
        ("test", test.check.status),
        ("semantic", semantic.check.status),
        ("legacy_skills", legacy_skills.check.status),
    ]
    .into_iter()
    .filter(|(_, s)| s.needs_action())
    .map(|(name, _)| name)
    .collect();

    DoctorReport {
        heal_version: env!("CARGO_PKG_VERSION"),
        project: project.display().to_string(),
        initialized,
        config,
        calibration,
        post_commit_hook,
        docs,
        test,
        semantic,
        legacy_skills,
        todo,
    }
}

fn check_config(project: &Path, initialized: bool) -> (ConfigCheck, Option<Config>) {
    if !initialized {
        let check = Check::new(
            Status::Todo,
            ".heal/config.toml is missing",
            Some("heal init"),
        );
        return (
            ConfigCheck {
                check,
                response_language: None,
            },
            None,
        );
    }
    match load_from_project(project) {
        Ok(cfg) => {
            let check = Check::new(Status::Ok, ".heal/config.toml loads", None);
            let response_language = cfg.project.response_language.clone();
            (
                ConfigCheck {
                    check,
                    response_language,
                },
                Some(cfg),
            )
        }
        Err(e) => {
            let check = Check::new(
                Status::Error,
                format!(".heal/config.toml does not load: {e}"),
                Some("fix the key named in the error, then re-run heal doctor"),
            );
            (
                ConfigCheck {
                    check,
                    response_language: None,
                },
                None,
            )
        }
    }
}

fn check_calibration(project: &Path, paths: &HealPaths, cfg: Option<&Config>) -> CalibrationCheck {
    let path = paths.calibration();
    if !path.exists() {
        return CalibrationCheck {
            check: Check::new(
                Status::Todo,
                ".heal/calibration.toml is missing",
                Some("heal calibrate --force"),
            ),
            facts: CalibrationFacts::default(),
        };
    }
    let calibration = match Calibration::load(&path) {
        Ok(c) => c,
        Err(e) => {
            return CalibrationCheck {
                check: Check::new(
                    Status::Error,
                    format!(".heal/calibration.toml does not load: {e}"),
                    Some("heal calibrate --force"),
                ),
                facts: CalibrationFacts::default(),
            };
        }
    };

    let meta = &calibration.meta;
    let commits_since = meta
        .calibrated_at_sha
        .as_deref()
        .and_then(|sha| git::commits_since(project, sha));
    let current_files = cfg.map(|c| LocObserver::from_config(c).scan(project).total_files());
    let fixed_since = read_fixed(&paths.findings_fixed()).ok().map(|fixed| {
        fixed
            .values()
            .filter(|f| f.fixed_at > meta.created_at)
            .count()
    });
    let clear_of_high = read_latest(&paths.findings_latest())
        .ok()
        .flatten()
        .map(|r| r.severity_counts.critical == 0 && r.severity_counts.high == 0);

    let facts = CalibrationFacts {
        created_at: Some(meta.created_at),
        calibrated_at_sha: meta.calibrated_at_sha.clone(),
        calibrated_at_files: Some(meta.codebase_files),
        current_files,
        commits_since,
        fixed_since,
        reasons: drift_reasons(
            commits_since,
            meta.codebase_files,
            current_files,
            clear_of_high,
            fixed_since,
        ),
    };
    let check = if facts.reasons.is_empty() {
        Check::new(Status::Ok, "calibration matches the codebase", None)
    } else {
        Check::new(
            Status::Todo,
            format!("calibration may be stale ({})", facts.reasons.join(", ")),
            Some("heal calibrate --force"),
        )
    };
    CalibrationCheck { check, facts }
}

/// The recalibration rules `/heal:setup` suggests `heal calibrate --force`
/// on. Any one firing is enough; the user still decides.
fn drift_reasons(
    commits_since: Option<usize>,
    calibrated_files: u32,
    current_files: Option<usize>,
    clear_of_high: Option<bool>,
    fixed_since: Option<usize>,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if commits_since.is_some_and(|n| n > STALE_COMMITS) {
        reasons.push("commits_since_calibration");
    }
    if let Some(now) = current_files {
        if calibrated_files > 0 {
            #[allow(clippy::cast_precision_loss)] // file counts fit an f64 exactly
            let drift =
                (now as f64 - f64::from(calibrated_files)).abs() / f64::from(calibrated_files);
            if drift > STALE_FILE_DRIFT {
                reasons.push("file_count_drift");
            }
        }
    }
    if clear_of_high == Some(true) && fixed_since.is_some_and(|n| n >= GRADUATED_FIXES) {
        reasons.push("graduated");
    }
    reasons
}

fn check_hook(project: &Path, initialized: bool) -> Check {
    let Some(hooks_dir) = git::hooks_dir(project) else {
        return Check::new(Status::Off, "not a git repository", None);
    };
    let hook = hooks_dir.join("post-commit");
    let fix = if initialized {
        "heal init"
    } else {
        "heal init (after the config step)"
    };
    match std::fs::read_to_string(&hook) {
        Ok(body) if body.contains(HEAL_HOOK_MARKER) => {
            Check::new(Status::Ok, "post-commit hook installed", None)
        }
        Ok(_) => Check::new(
            Status::Warn,
            format!(
                "{} exists without the HEAL marker; add `heal hook commit || true` to it by hand",
                hook.display()
            ),
            None,
        ),
        Err(_) => Check::new(Status::Todo, "post-commit hook is not installed", Some(fix)),
    }
}

fn check_docs(project: &Path, cfg: &Config) -> DocsCheck {
    let docs = &cfg.features.docs;
    let pairs_path = docs.pairs_path.clone();
    if !docs.enabled {
        return DocsCheck {
            check: Check::new(Status::Off, "[features.docs] is disabled", None),
            enabled: false,
            pairs_path,
            pairs: None,
            missing_paths: 0,
        };
    }
    let (check, pairs, missing_paths) = match DocPairsFile::read(project, &pairs_path) {
        Ok(Some(file)) => {
            let missing = file.integrity_check(project).len();
            let check = if missing == 0 {
                Check::new(
                    Status::Ok,
                    format!("{} doc pairs in {pairs_path}", file.pairs.len()),
                    None,
                )
            } else {
                Check::new(
                    Status::Todo,
                    format!("{missing} paths in {pairs_path} no longer exist"),
                    Some("/heal:setup"),
                )
            };
            (check, Some(file.pairs.len()), missing)
        }
        Ok(None) => (
            Check::new(
                Status::Todo,
                format!("{pairs_path} is missing or has an older schema"),
                Some("/heal:setup"),
            ),
            None,
            0,
        ),
        Err(e) => (
            Check::new(
                Status::Error,
                format!("{pairs_path} does not load: {e}"),
                Some("/heal:setup"),
            ),
            None,
            0,
        ),
    };
    DocsCheck {
        check,
        enabled: true,
        pairs_path,
        pairs,
        missing_paths,
    }
}

fn check_test(project: &Path, cfg: &Config) -> TestCheck {
    let test = &cfg.features.test;
    let coverage = &test.coverage;
    let lcov_found: Vec<String> = coverage
        .lcov_paths
        .iter()
        .filter(|p| project.join(p).is_file())
        .cloned()
        .collect();
    let check = if !test.enabled {
        Check::new(Status::Off, "[features.test] is disabled", None)
    } else if !coverage.enabled {
        Check::new(
            Status::Ok,
            "[features.test] is enabled; coverage ingestion is off",
            None,
        )
    } else if lcov_found.is_empty() {
        Check::new(
            Status::Todo,
            "coverage is enabled but no lcov_paths entry exists",
            Some("/heal:setup"),
        )
    } else {
        Check::new(
            Status::Ok,
            format!("coverage read from {}", lcov_found.join(", ")),
            None,
        )
    };
    TestCheck {
        check,
        enabled: test.enabled,
        coverage_enabled: coverage.enabled,
        lcov_found,
        post_commit_refresh: coverage.post_commit_refresh.is_some(),
    }
}

/// Where a Jev key would be read from, without contacting the API.
fn key_source() -> Option<&'static str> {
    let file = default_credentials_path();
    match resolve(file.as_deref()) {
        Ok(Some(key)) => Some(match key.source {
            KeySource::Env(_) => "environment",
            KeySource::File(_) => "credentials_file",
        }),
        Ok(None) | Err(_) => None,
    }
}

fn check_semantic(
    paths: &HealPaths,
    cfg: &Config,
    key_source: Option<&'static str>,
) -> SemanticCheck {
    let enabled = cfg.features.semantic.enabled;
    let concepts = Concepts::load(&paths.concepts());
    let concept_count = concepts
        .as_ref()
        .ok()
        .and_then(|c| c.as_ref().map(|c| c.concepts.len()));
    let check = if !enabled {
        Check::new(Status::Off, "[features.semantic] is disabled", None)
    } else if let Err(e) = &concepts {
        Check::new(
            Status::Error,
            format!(".heal/concepts.toml does not load: {e}"),
            Some("/heal:setup"),
        )
    } else if key_source.is_none() {
        Check::new(
            Status::Todo,
            "no Jev API key is configured",
            Some("heal auth jev set"),
        )
    } else if concept_count.is_none() {
        Check::new(
            Status::Todo,
            ".heal/concepts.toml is missing; the concept tasks plan nothing",
            Some("/heal:setup"),
        )
    } else {
        Check::new(
            Status::Ok,
            "enabled with a key and a concept vocabulary",
            None,
        )
    };
    SemanticCheck {
        check,
        enabled,
        concepts: concept_count,
        key_source,
    }
}

fn check_legacy_skills(project: &Path) -> LegacySkillsCheck {
    let paths: Vec<String> = legacy_skills::find(project)
        .into_iter()
        .map(|p| p.strip_prefix(project).unwrap_or(&p).display().to_string())
        .collect();
    let check = if paths.is_empty() {
        Check::new(Status::Ok, "no skills from older HEAL versions", None)
    } else {
        Check::new(
            Status::Todo,
            format!(
                "{} skill directories from older HEAL versions; skills now ship as the heal plugin",
                paths.len()
            ),
            Some("heal skills uninstall"),
        )
    };
    LegacySkillsCheck { check, paths }
}

fn print_report(report: &DoctorReport) {
    println!("heal {}  {}", report.heal_version, report.project);
    println!();
    let rows: [(&str, &Check); 7] = [
        ("config", &report.config.check),
        ("calibration", &report.calibration.check),
        ("post-commit hook", &report.post_commit_hook),
        ("docs", &report.docs.check),
        ("test", &report.test.check),
        ("semantic", &report.semantic.check),
        ("legacy skills", &report.legacy_skills.check),
    ];
    for (name, check) in rows {
        let fix = check
            .fix
            .as_deref()
            .map(|f| format!("  → {f}"))
            .unwrap_or_default();
        println!(
            "  {:<5}  {:<16}  {}{fix}",
            check.status.label(),
            name,
            check.detail
        );
    }
    println!();
    if report.todo.is_empty() {
        println!(
            "Nothing to set up. Optional features marked `off` can be enabled with /heal:setup."
        );
    } else {
        println!("Run /heal:setup to work through the todo items.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::hook_install;
    use crate::core::findings_cache::FixedFinding;
    use crate::test_support::init_project_with_config;
    use tempfile::TempDir;

    #[test]
    fn uninitialized_project_lists_config_and_calibration_as_todo() {
        let dir = TempDir::new().unwrap();
        crate::test_support::init_repo(dir.path());
        let report = diagnose(dir.path());
        assert!(!report.initialized);
        assert_eq!(report.config.check.status, Status::Todo);
        assert_eq!(report.calibration.check.status, Status::Todo);
        assert_eq!(report.post_commit_hook.status, Status::Todo);
        assert_eq!(report.docs.check.status, Status::Off);
        assert_eq!(report.test.check.status, Status::Off);
        assert_eq!(report.semantic.check.status, Status::Off);
        assert_eq!(
            &report.todo[..3],
            &["config", "calibration", "post_commit_hook"]
        );
    }

    #[test]
    fn initialized_project_with_hook_and_calibration_is_clean() {
        let dir = TempDir::new().unwrap();
        let paths = init_project_with_config(dir.path(), "fn main() {}\n");
        hook_install::install(dir.path(), false).unwrap();
        crate::commands::calibrate::run(dir.path(), true, true).unwrap();

        let report = diagnose(dir.path());
        assert!(report.initialized);
        assert_eq!(report.config.check.status, Status::Ok);
        assert_eq!(report.post_commit_hook.status, Status::Ok);
        assert_eq!(report.calibration.check.status, Status::Ok);
        assert_eq!(report.calibration.facts.commits_since, Some(0));
        assert!(report.todo.is_empty(), "todo: {:?}", report.todo);
        assert!(paths.calibration().exists());
    }

    #[test]
    fn broken_config_is_an_error_and_features_fall_back_to_off() {
        let dir = TempDir::new().unwrap();
        let paths = init_project_with_config(dir.path(), "fn main() {}\n");
        std::fs::write(paths.config(), "[unknown_section]\nx = 1\n").unwrap();
        let report = diagnose(dir.path());
        assert_eq!(report.config.check.status, Status::Error);
        assert_eq!(report.docs.check.status, Status::Off);
        assert!(report.todo.contains(&"config"));
    }

    #[test]
    fn foreign_post_commit_hook_is_a_warning() {
        let dir = TempDir::new().unwrap();
        init_project_with_config(dir.path(), "fn main() {}\n");
        let hooks = git::hooks_dir(dir.path()).unwrap();
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(hooks.join("post-commit"), "#!/bin/sh\necho mine\n").unwrap();
        let report = diagnose(dir.path());
        assert_eq!(report.post_commit_hook.status, Status::Warn);
        assert!(!report.todo.contains(&"post_commit_hook"));
    }

    #[test]
    fn enabled_docs_without_pairs_file_is_todo() {
        let dir = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.features.docs.enabled = true;
        let report = check_docs(dir.path(), &cfg);
        assert_eq!(report.check.status, Status::Todo);
        assert_eq!(report.check.fix.as_deref(), Some("/heal:setup"));
        assert!(report.pairs.is_none());
    }

    #[test]
    fn coverage_without_lcov_is_todo_and_found_paths_are_listed() {
        let dir = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.features.test.enabled = true;
        cfg.features.test.coverage.enabled = true;
        assert_eq!(check_test(dir.path(), &cfg).check.status, Status::Todo);

        std::fs::write(dir.path().join("lcov.info"), "TN:\n").unwrap();
        let report = check_test(dir.path(), &cfg);
        assert_eq!(report.check.status, Status::Ok);
        assert_eq!(report.lcov_found, vec!["lcov.info".to_owned()]);
    }

    #[test]
    fn semantic_needs_a_key_before_concepts() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        let mut cfg = Config::default();
        cfg.features.semantic.enabled = true;

        let no_key = check_semantic(&paths, &cfg, None);
        assert_eq!(no_key.check.status, Status::Todo);
        assert_eq!(no_key.check.fix.as_deref(), Some("heal auth jev set"));

        let no_concepts = check_semantic(&paths, &cfg, Some("environment"));
        assert_eq!(no_concepts.check.status, Status::Todo);
        assert_eq!(no_concepts.check.fix.as_deref(), Some("/heal:setup"));

        cfg.features.semantic.enabled = false;
        assert_eq!(check_semantic(&paths, &cfg, None).check.status, Status::Off);
    }

    #[test]
    fn drift_rules_fire_independently() {
        assert!(drift_reasons(Some(10), 100, Some(110), Some(false), Some(0)).is_empty());
        assert_eq!(
            drift_reasons(Some(201), 100, Some(100), None, None),
            vec!["commits_since_calibration"]
        );
        assert_eq!(
            drift_reasons(None, 100, Some(121), None, None),
            vec!["file_count_drift"]
        );
        assert_eq!(
            drift_reasons(None, 100, Some(79), None, None),
            vec!["file_count_drift"]
        );
        assert_eq!(
            drift_reasons(None, 100, None, Some(true), Some(10)),
            vec!["graduated"]
        );
        assert!(drift_reasons(None, 100, None, Some(true), Some(9)).is_empty());
    }

    #[test]
    fn fixes_before_calibration_do_not_count() {
        let dir = TempDir::new().unwrap();
        let paths = init_project_with_config(dir.path(), "fn main() {}\n");
        crate::commands::calibrate::run(dir.path(), true, true).unwrap();
        let created = Calibration::load(&paths.calibration())
            .unwrap()
            .meta
            .created_at;
        let mut fixed = crate::core::findings_cache::FixedMap::new();
        for (i, at) in [
            created - chrono::Duration::days(1),
            created + chrono::Duration::days(1),
        ]
        .into_iter()
        .enumerate()
        {
            let id = format!("ccn:a.rs:f{i}:0000000000000000");
            fixed.insert(
                id.clone(),
                FixedFinding {
                    finding_id: id,
                    commit_sha: "0".repeat(40),
                    fixed_at: at,
                },
            );
        }
        std::fs::create_dir_all(paths.findings_dir()).unwrap();
        std::fs::write(
            paths.findings_fixed(),
            serde_json::to_string(&fixed).unwrap(),
        )
        .unwrap();
        let report = diagnose(dir.path());
        assert_eq!(report.calibration.facts.fixed_since, Some(1));
    }

    #[test]
    fn legacy_skill_dirs_are_reported_relative_to_the_project() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude/skills/heal-code-review")).unwrap();
        let report = check_legacy_skills(dir.path());
        assert_eq!(report.check.status, Status::Todo);
        assert_eq!(
            report.paths,
            vec![".claude/skills/heal-code-review".to_owned()]
        );
    }
}
