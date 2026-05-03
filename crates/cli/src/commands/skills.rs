//! `heal skills <install|update|status|uninstall>` — manage the bundled
//! Claude skill set under `<project>/.claude/skills/`.
//!
//! Each top-level directory under the embedded tree (`heal-cli`,
//! `heal-config`, `heal-code-review`, `heal-code-patch`) is extracted
//! to a sibling under `.claude/skills/`. The install pass also reaches
//! into `.claude/settings.json` to sweep legacy `heal hook edit` /
//! `heal hook stop` registrations left over from earlier versions.
//! No new hooks are registered.
//!
//! `install` is the safe default (skips existing files), `update` is
//! drift-aware (overwrites unmodified assets, leaves user edits alone
//! unless `--force`), and `status` reads each SKILL.md's frontmatter
//! `metadata:` block to surface the installed version + drift list.
//! There is no sidecar manifest — the bundled bytes are the source of
//! truth, and on-disk state alone determines drift.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::claude_settings::{self, WireReport, WriteAction};
use crate::cli::SkillsAction;
use crate::core::HealPaths;
use crate::skill_assets::{
    self, skills_dest, ExtractMode, ExtractStats, ExtractSummary, INSTALL_SOURCE_BUNDLED,
};

pub fn run(project: &Path, action: SkillsAction) -> Result<()> {
    let paths = HealPaths::new(project);
    let dest = skills_dest(project);
    match action {
        SkillsAction::Install { force, json } => install(project, &paths, &dest, force, json),
        SkillsAction::Update { force, json } => update(project, &paths, &dest, force, json),
        SkillsAction::Status { json } => {
            status(&dest, json);
            Ok(())
        }
        SkillsAction::Uninstall { json } => uninstall(project, &paths, &dest, json),
    }
}

fn install(
    project: &Path,
    _paths: &HealPaths,
    dest: &Path,
    force: bool,
    as_json: bool,
) -> Result<()> {
    let mode = if force {
        ExtractMode::InstallForce
    } else {
        ExtractMode::InstallSafe
    };
    let stats = skill_assets::extract(dest, mode)?;
    let wire = claude_settings::wire(project)?;
    let version = skill_assets::bundled_version();
    if as_json {
        super::emit_json(&SkillsActionReport {
            action: SkillsActionKind::Installed,
            dest: dest.display().to_string(),
            version: &version,
            source: INSTALL_SOURCE_BUNDLED,
            files: stats.summary(),
            user_modified_paths: &stats.user_modified,
            claude: wire,
        });
        return Ok(());
    }
    println!("skills {} at {}", install_verb(force), dest.display());
    println!("  version: {version}");
    println!("  source:  {INSTALL_SOURCE_BUNDLED}");
    print_extract_summary(&stats);
    print_wire_summary(wire);
    Ok(())
}

fn update(
    project: &Path,
    _paths: &HealPaths,
    dest: &Path,
    force: bool,
    as_json: bool,
) -> Result<()> {
    let stats = skill_assets::extract(dest, ExtractMode::Update { force })?;
    let wire = claude_settings::wire(project)?;
    let version = skill_assets::bundled_version();
    if as_json {
        super::emit_json(&SkillsActionReport {
            action: SkillsActionKind::Updated,
            dest: dest.display().to_string(),
            version: &version,
            source: INSTALL_SOURCE_BUNDLED,
            files: stats.summary(),
            user_modified_paths: &stats.user_modified,
            claude: wire,
        });
        return Ok(());
    }
    println!("skills updated at {}", dest.display());
    println!("  version: {version}");
    print_extract_summary(&stats);
    print_wire_summary(wire);
    if !stats.user_modified.is_empty() && !force {
        println!(
            "  hint: {} file(s) skipped due to local edits — pass `--force` to overwrite.",
            stats.user_modified.len()
        );
    }
    Ok(())
}

/// Snapshot for `heal skills status`. Read-only: reads every bundled
/// skill's SKILL.md from disk to surface installed version + drift.
fn status(dest: &Path, as_json: bool) {
    let bundled = skill_assets::bundled_version();

    if !dest.exists() {
        if as_json {
            super::emit_json(&StatusReport {
                state: StatusState::NotInstalled,
                dest: dest.display().to_string(),
                bundled: Some(bundled),
                ..StatusReport::default()
            });
        } else {
            println!("skills: not installed (run `heal skills install`)");
        }
        return;
    }

    let installed = read_installed_version(dest);
    let drift = drifted_assets(dest);

    let cmp = installed
        .as_deref()
        .map_or(VersionCmp::BundledNewer, |v| compare_versions(v, &bundled));

    if as_json {
        super::emit_json(&StatusReport {
            state: StatusState::Installed,
            dest: dest.display().to_string(),
            installed: installed.clone(),
            bundled: Some(bundled),
            source: Some(INSTALL_SOURCE_BUNDLED.to_string()),
            version_status: Some(cmp),
            drift,
        });
        return;
    }

    println!("skills: installed at {}", dest.display());
    println!(
        "  installed: {}",
        installed
            .as_deref()
            .unwrap_or("(unknown — pre-metadata install)"),
    );
    println!("  bundled:   {bundled}");
    println!("  source:    {INSTALL_SOURCE_BUNDLED}");

    let label = match cmp {
        VersionCmp::Match => "up-to-date",
        VersionCmp::BundledNewer => "bundled-newer (run `heal skills update`)",
        VersionCmp::InstalledNewer => "installed-newer (binary downgrade?)",
    };
    println!("  status:    {label}");

    if drift.is_empty() {
        return;
    }
    println!("  drift:     {} file(s) edited locally", drift.len());
    for p in drift {
        println!("    - {p}");
    }
}

fn uninstall(project: &Path, _paths: &HealPaths, dest: &Path, as_json: bool) -> Result<()> {
    let removed = remove_installed_skills(dest)?;
    let claude_settings::UnregisterReport { legacy_swept } = claude_settings::unregister(project)?;

    let action = if removed.is_empty() && !legacy_swept {
        UninstallAction::Noop
    } else {
        UninstallAction::Removed
    };

    if as_json {
        super::emit_json(&UninstallReport {
            action,
            dest: dest.display().to_string(),
            skills_removed: removed,
            legacy_swept,
        });
        return Ok(());
    }
    match action {
        UninstallAction::Removed => {
            if removed.is_empty() {
                println!("removed legacy plugin/marketplace install layout");
            } else {
                println!(
                    "removed {} skill(s) under {}",
                    removed.len(),
                    dest.display()
                );
                for s in &removed {
                    println!("  - {s}");
                }
                if legacy_swept {
                    println!("  also removed legacy plugin/marketplace install layout");
                }
            }
        }
        UninstallAction::Noop => println!("skills not installed; nothing to do"),
    }
    Ok(())
}

/// Walk every bundled skill name and remove the corresponding directory
/// from `dest` if present. Untracked sibling skills survive — we never
/// recursively nuke `.claude/skills/` because the user may have other
/// skills there.
fn remove_installed_skills(dest: &Path) -> Result<Vec<String>> {
    let mut removed: Vec<String> = Vec::new();
    for name in skill_assets::bundled_skill_names() {
        let target = dest.join(&name);
        if target.exists() {
            std::fs::remove_dir_all(&target)
                .with_context(|| format!("removing {}", target.display()))?;
            removed.push(name);
        }
    }
    let _ = crate::core::fs::remove_dir_if_empty(dest);
    Ok(removed)
}

/// Read the installed `heal-version` from the first bundled skill's
/// SKILL.md whose frontmatter carries the metadata block. Returns
/// `None` for installs that pre-date the frontmatter convention or for
/// dest dirs that don't carry any bundled skill.
fn read_installed_version(dest: &Path) -> Option<String> {
    for name in skill_assets::bundled_skill_names() {
        let path = dest.join(&name).join("SKILL.md");
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(version) = skill_assets::read_installed_version(&body) {
            return Some(version);
        }
    }
    None
}

/// Walk every bundled file and return the relative paths whose
/// canonical (metadata-stripped) on-disk bytes diverge from the
/// bundled raw bytes — i.e. the user has hand-edited them.
fn drifted_assets(dest: &Path) -> Vec<String> {
    use include_dir::DirEntry;

    fn walk_dir(dir: &include_dir::Dir<'_>, dest: &Path, rel_prefix: &Path, out: &mut Vec<String>) {
        for entry in dir.entries() {
            match entry {
                DirEntry::Dir(child) => walk_dir(child, dest, child.path(), out),
                DirEntry::File(file) => {
                    let target = dest.join(file.path());
                    let Ok(on_disk) = std::fs::read(&target) else {
                        continue;
                    };
                    let canonical = if file.path().file_name().is_some_and(|n| n == "SKILL.md") {
                        match std::str::from_utf8(&on_disk) {
                            Ok(text) => skill_assets::strip_skill_metadata(text).into_bytes(),
                            Err(_) => on_disk.clone(),
                        }
                    } else {
                        on_disk.clone()
                    };
                    if canonical != file.contents() {
                        out.push(
                            file.path()
                                .components()
                                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                                .collect::<Vec<_>>()
                                .join("/"),
                        );
                    }
                    let _ = rel_prefix;
                }
            }
        }
    }

    let mut drift: Vec<String> = Vec::new();
    walk_dir(
        &skill_assets::SKILLS_DIR,
        dest,
        skill_assets::SKILLS_DIR.path(),
        &mut drift,
    );
    drift.sort();
    drift
}

fn install_verb(force: bool) -> &'static str {
    if force {
        "force-installed"
    } else {
        "extracted"
    }
}

fn print_extract_summary(stats: &ExtractStats) {
    let s: ExtractSummary = stats.summary();
    println!(
        "  files:   added {} | updated {} | unchanged {} | skipped {} | local-edits {}",
        s.added, s.updated, s.unchanged, s.skipped, s.user_modified
    );
    if !stats.user_modified.is_empty() {
        for p in &stats.user_modified {
            println!("    skipped (local edit): {p}");
        }
    }
}

fn print_wire_summary(report: WireReport) {
    println!("  claude:  settings {}", wire_verb(report.settings));
}

fn wire_verb(action: WriteAction) -> &'static str {
    match action {
        WriteAction::Created => "created",
        WriteAction::Updated => "updated",
        WriteAction::Unchanged => "unchanged",
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum SkillsActionKind {
    Installed,
    Updated,
}

#[derive(Debug, Serialize)]
struct SkillsActionReport<'a> {
    action: SkillsActionKind,
    dest: String,
    version: &'a str,
    source: &'a str,
    files: ExtractSummary,
    user_modified_paths: &'a [String],
    claude: WireReport,
}

/// Three reachable states for `heal skills status`. `Default` falls
/// back to `NotInstalled` because that's the variant `StatusReport`'s
/// struct-update pattern (`..StatusReport::default()`) coexists with —
/// every concrete construction overrides `state` explicitly.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
enum StatusState {
    #[default]
    NotInstalled,
    Installed,
}

#[derive(Debug, Default, Serialize)]
struct StatusReport {
    state: StatusState,
    dest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    installed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bundled: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version_status: Option<VersionCmp>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    drift: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum UninstallAction {
    Removed,
    Noop,
}

#[derive(Debug, Serialize)]
struct UninstallReport {
    action: UninstallAction,
    dest: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    skills_removed: Vec<String>,
    /// `true` when uninstall also swept artifacts from the pre-`feat(skills)!`
    /// install layout (plugin tree, marketplace.json, legacy settings keys).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    legacy_swept: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum VersionCmp {
    #[serde(rename = "up_to_date")]
    Match,
    BundledNewer,
    InstalledNewer,
}

/// Best-effort dotted-numeric version compare. Strips the semver
/// pre-release / build-metadata suffix (`0.1.0-rc1`, `0.1.0+sha.abc`) before
/// parsing so a pre-release binary doesn't get classified as drift against
/// an install marker recorded with the same release version.
fn compare_versions(installed: &str, bundled: &str) -> VersionCmp {
    let parse = |s: &str| -> Option<Vec<u32>> {
        let core = s.split(['-', '+']).next().unwrap_or(s);
        core.split('.').map(|p| p.parse::<u32>().ok()).collect()
    };
    match (parse(installed), parse(bundled)) {
        (Some(i), Some(b)) => {
            use std::cmp::Ordering::{Equal, Greater, Less};
            match i.cmp(&b) {
                Less => VersionCmp::BundledNewer,
                Greater => VersionCmp::InstalledNewer,
                Equal => VersionCmp::Match,
            }
        }
        _ if installed == bundled => VersionCmp::Match,
        _ => VersionCmp::BundledNewer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn version_cmp_handles_dotted_semver() {
        assert_eq!(compare_versions("0.1.0", "0.1.0"), VersionCmp::Match);
        assert_eq!(compare_versions("0.1.0", "0.2.0"), VersionCmp::BundledNewer);
        assert_eq!(
            compare_versions("0.2.0", "0.1.0"),
            VersionCmp::InstalledNewer
        );
    }

    #[test]
    fn version_cmp_falls_back_on_unknown() {
        assert_eq!(compare_versions("unknown", "unknown"), VersionCmp::Match);
        assert_eq!(
            compare_versions("unknown", "0.1.0"),
            VersionCmp::BundledNewer
        );
    }

    #[test]
    fn drift_detection_flags_user_edits() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("skills");
        skill_assets::extract(&dest, ExtractMode::InstallSafe).unwrap();
        // No drift right after install — only heal's own metadata stamp.
        assert!(drifted_assets(&dest).is_empty());

        // Tamper with a known-shipped skill file.
        std::fs::write(dest.join("heal-code-patch/SKILL.md"), "tampered\n").unwrap();
        let drift = drifted_assets(&dest);
        assert!(drift.iter().any(|p| p == "heal-code-patch/SKILL.md"));
    }

    #[test]
    fn read_installed_version_recovers_from_skill_md() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("skills");
        skill_assets::extract(&dest, ExtractMode::InstallSafe).unwrap();
        let installed = read_installed_version(&dest);
        assert_eq!(
            installed.as_deref(),
            Some(skill_assets::bundled_version().as_str())
        );
    }

    #[test]
    fn uninstall_removes_installed_skills() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        let dest = skills_dest(project);
        install(project, &paths, &dest, false, false).unwrap();
        assert!(dest.join("heal-cli/SKILL.md").exists());

        uninstall(project, &paths, &dest, false).unwrap();
        assert!(!dest.join("heal-cli/SKILL.md").exists());
    }

    #[test]
    fn install_does_not_register_claude_hooks() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        let dest = skills_dest(project);
        install(project, &paths, &dest, false, false).unwrap();
        assert!(!project.join(".claude/settings.json").exists());
    }

    #[test]
    fn install_sweeps_legacy_heal_hook_entries() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        let settings = project.join(".claude/settings.json");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(
            &settings,
            r#"{
              "hooks": {
                "PostToolUse": [
                  { "matcher": "Edit|Write|MultiEdit",
                    "hooks": [
                      { "type": "command", "command": "heal hook edit" },
                      { "type": "command", "command": "echo edit" }
                    ]
                  }
                ]
              }
            }"#,
        )
        .unwrap();
        let dest = skills_dest(project);
        install(project, &paths, &dest, false, false).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        let cmds: Vec<&str> = v["hooks"]["PostToolUse"][0]["hooks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["command"].as_str().unwrap())
            .collect();
        assert_eq!(cmds, vec!["echo edit"]);
    }

    #[test]
    fn uninstall_when_nothing_installed_is_noop() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        let dest = skills_dest(project);
        uninstall(project, &paths, &dest, false).unwrap();
        assert!(!project.join(".claude/settings.json").exists());
    }

    #[test]
    fn uninstall_sweeps_legacy_install_with_no_new_install_present() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();

        let plugin_tree = project.join(".claude/plugins/heal");
        std::fs::create_dir_all(&plugin_tree).unwrap();
        std::fs::write(plugin_tree.join("plugin.json"), "{}").unwrap();
        let market = project.join(".claude-plugin/marketplace.json");
        std::fs::create_dir_all(market.parent().unwrap()).unwrap();
        std::fs::write(&market, "{}").unwrap();
        std::fs::write(
            project.join(".claude/settings.json"),
            r#"{"enabledPlugins":{"heal@heal-local":true},"extraKnownMarketplaces":{"heal-local":{"source":{"source":"file","path":"./.claude-plugin/marketplace.json"}}}}"#,
        )
        .unwrap();

        let dest = skills_dest(project);
        uninstall(project, &paths, &dest, false).unwrap();

        assert!(!plugin_tree.exists());
        assert!(!market.exists());
        assert!(
            !project.join(".claude/settings.json").exists(),
            "legacy-only settings should collapse to deletion"
        );
    }
}
