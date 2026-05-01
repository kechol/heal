//! `heal skills <install|update|status|uninstall>` — manage the bundled
//! Claude skill set under `<project>/.claude/skills/`.
//!
//! Each top-level directory under the embedded tree (`heal-cli`,
//! `heal-config`, `heal-code-review`, `heal-code-patch`) is extracted
//! to a sibling under `.claude/skills/`. The install also merges
//! HEAL's hook commands into `.claude/settings.json` so the post-tool-use
//! and Stop events feed the HEAL event log.
//!
//! `install` is the safe default (skips existing files), `update` is
//! drift-aware (overwrites unchanged-since-install assets, leaves user
//! edits alone unless `--force`), and `status` exposes the bundled vs.
//! installed version plus any drift surfaced by the manifest.

use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::claude_settings::{self, WireReport, WriteAction};
use crate::cli::SkillsAction;
use crate::core::HealPaths;
use crate::skill_assets::{
    self, skills_dest, ExtractMode, ExtractStats, ExtractSummary, InstallManifest,
};

pub fn run(project: &Path, action: SkillsAction) -> Result<()> {
    let paths = HealPaths::new(project);
    let dest = skills_dest(project);
    match action {
        SkillsAction::Install { force, json } => install(project, &paths, &dest, force, json),
        SkillsAction::Update { force, json } => update(project, &paths, &dest, force, json),
        SkillsAction::Status { json } => {
            status(&paths, &dest, json);
            Ok(())
        }
        SkillsAction::Uninstall { json } => uninstall(project, &paths, &dest, json),
    }
}

fn install(
    project: &Path,
    paths: &HealPaths,
    dest: &Path,
    force: bool,
    as_json: bool,
) -> Result<()> {
    let mode = if force {
        ExtractMode::InstallForce
    } else {
        ExtractMode::InstallSafe
    };
    let manifest_path = paths.skills_install_manifest();
    let (stats, manifest) = skill_assets::extract(dest, &manifest_path, mode)?;
    let wire = claude_settings::wire(project)?;
    if as_json {
        super::emit_json(&SkillsActionReport::new(
            SkillsActionKind::Installed,
            dest,
            &manifest,
            &stats,
            wire,
        ));
        return Ok(());
    }
    println!("skills {} at {}", install_verb(force), dest.display());
    println!("  version: {}", manifest.heal_version);
    println!("  source:  {}", manifest.source);
    print_extract_summary(&stats);
    print_wire_summary(wire);
    Ok(())
}

fn update(
    project: &Path,
    paths: &HealPaths,
    dest: &Path,
    force: bool,
    as_json: bool,
) -> Result<()> {
    let manifest_path = paths.skills_install_manifest();
    let (stats, manifest) =
        skill_assets::extract(dest, &manifest_path, ExtractMode::Update { force })?;
    let wire = claude_settings::wire(project)?;
    if as_json {
        super::emit_json(&SkillsActionReport::new(
            SkillsActionKind::Updated,
            dest,
            &manifest,
            &stats,
            wire,
        ));
        return Ok(());
    }
    println!("skills updated at {}", dest.display());
    println!("  version: {}", manifest.heal_version);
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

/// Snapshot for `heal skills status`. Read-only: never touches the
/// manifest, never invokes `extract`.
fn status(paths: &HealPaths, dest: &Path, as_json: bool) {
    let manifest_path = paths.skills_install_manifest();
    let bundled = skill_assets::bundled_version();

    let Some(manifest) = InstallManifest::load(&manifest_path) else {
        let state = if dest.exists() {
            StatusState::NoManifest
        } else {
            StatusState::NotInstalled
        };
        if as_json {
            super::emit_json(&StatusReport {
                state,
                dest: dest.display().to_string(),
                bundled: Some(bundled),
                ..StatusReport::default()
            });
            return;
        }
        match state {
            StatusState::NotInstalled => {
                println!("skills: not installed (run `heal skills install`)");
            }
            StatusState::NoManifest => {
                println!(
                    "skills: directory exists at {} but no manifest ({})",
                    dest.display(),
                    manifest_path.display()
                );
                println!("  bundled: {bundled}");
                println!("  hint:    run `heal skills update --force` to refresh metadata.");
            }
            StatusState::Installed => unreachable!(),
        }
        return;
    };

    let cmp = compare_versions(&manifest.heal_version, &bundled);
    let drift = drifted_assets(dest, &manifest);

    if as_json {
        super::emit_json(&StatusReport {
            state: StatusState::Installed,
            dest: dest.display().to_string(),
            installed: Some(manifest.heal_version.clone()),
            bundled: Some(bundled),
            installed_at: Some(manifest.installed_at.to_rfc3339()),
            source: Some(manifest.source.clone()),
            version_status: Some(cmp),
            drift,
        });
        return;
    }

    println!("skills: installed at {}", dest.display());
    println!("  installed: {}", manifest.heal_version);
    println!("  bundled:   {bundled}");
    println!("  installed at: {}", manifest.installed_at.to_rfc3339());
    println!("  source:    {}", manifest.source);

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

fn uninstall(project: &Path, paths: &HealPaths, dest: &Path, as_json: bool) -> Result<()> {
    let manifest_path = paths.skills_install_manifest();
    let removed = remove_installed_skills(dest, &manifest_path)?;
    let manifest_existed = manifest_path.exists();
    if manifest_existed {
        std::fs::remove_file(&manifest_path)
            .with_context(|| format!("removing {}", manifest_path.display()))?;
    }
    let claude_settings::UnregisterReport { legacy_swept } = claude_settings::unregister(project)?;

    let action = if removed.is_empty() && !manifest_existed && !legacy_swept {
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
            if removed.is_empty() && !manifest_existed {
                println!("removed legacy plugin/marketplace install layout");
            } else if removed.is_empty() {
                println!("removed install manifest; no skill files were present");
            } else {
                println!(
                    "removed {} skill(s) under {}",
                    removed.len(),
                    dest.display()
                );
                for s in &removed {
                    println!("  - {s}");
                }
            }
            if legacy_swept && !removed.is_empty() {
                println!("  also removed legacy plugin/marketplace install layout");
            }
        }
        UninstallAction::Noop => println!("skills not installed; nothing to do"),
    }
    Ok(())
}

/// Remove every skill subdirectory recorded in the manifest. Returns
/// the set of skill names that were actually removed (lexicographic
/// order). Untracked files in `dest` are left alone — we never recursively
/// nuke `.claude/skills/` because the user may have other skills there.
fn remove_installed_skills(dest: &Path, manifest_path: &Path) -> Result<Vec<String>> {
    let Some(manifest) = InstallManifest::load(manifest_path) else {
        return Ok(Vec::new());
    };
    let skill_names: BTreeSet<String> = manifest
        .assets
        .keys()
        .filter_map(|rel| rel.split('/').next().map(str::to_string))
        .collect();

    let mut removed: Vec<String> = Vec::new();
    for name in &skill_names {
        let target = dest.join(name);
        if target.exists() {
            std::fs::remove_dir_all(&target)
                .with_context(|| format!("removing {}", target.display()))?;
            removed.push(name.clone());
        }
    }
    // If `.claude/skills/` is now empty (we owned every entry), remove it
    // too. Otherwise leave it for whoever else lives there.
    let _ = crate::core::fs::remove_dir_if_empty(dest);
    Ok(removed)
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

/// Stable JSON contract for `heal skills install --json` and
/// `heal skills update --json`. The `action` field discriminates the
/// two paths so callers can branch without parsing prose.
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

impl<'a> SkillsActionReport<'a> {
    fn new(
        action: SkillsActionKind,
        dest: &Path,
        manifest: &'a InstallManifest,
        stats: &'a ExtractStats,
        wire: WireReport,
    ) -> Self {
        Self {
            action,
            dest: dest.display().to_string(),
            version: &manifest.heal_version,
            source: &manifest.source,
            files: stats.summary(),
            user_modified_paths: &stats.user_modified,
            claude: wire,
        }
    }
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
    NoManifest,
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
    installed_at: Option<String>,
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
    /// `true` when uninstall also swept artefacts from the pre-`feat(skills)!`
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

/// Walk the manifest's recorded assets and return relative paths whose
/// on-disk fingerprint no longer matches the install record.
fn drifted_assets(dest: &Path, manifest: &InstallManifest) -> Vec<String> {
    let mut drift: Vec<String> = Vec::new();
    for (rel, fp) in &manifest.assets {
        let p: PathBuf = dest.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        if &skill_assets::fingerprint(&bytes) != fp {
            drift.push(rel.clone());
        }
    }
    drift
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
        let manifest_path = dir.path().join("skills-install.json");
        let (_, manifest) =
            skill_assets::extract(&dest, &manifest_path, ExtractMode::InstallSafe).unwrap();
        // No drift right after install.
        assert!(drifted_assets(&dest, &manifest).is_empty());

        // Tamper with a known-shipped skill file.
        std::fs::write(dest.join("heal-code-patch/SKILL.md"), "tampered\n").unwrap();
        let drift = drifted_assets(&dest, &manifest);
        assert!(drift.iter().any(|p| p == "heal-code-patch/SKILL.md"));
    }

    #[test]
    fn uninstall_removes_installed_skills_and_manifest() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        let dest = skills_dest(project);
        install(project, &paths, &dest, false, false).unwrap();
        assert!(dest.join("heal-cli/SKILL.md").exists());
        assert!(paths.skills_install_manifest().exists());

        uninstall(project, &paths, &dest, false).unwrap();
        assert!(!dest.join("heal-cli/SKILL.md").exists());
        assert!(!paths.skills_install_manifest().exists());
    }

    #[test]
    fn install_writes_settings_hooks() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        let dest = skills_dest(project);
        install(project, &paths, &dest, false, false).unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(project.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            settings["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
            "heal hook edit"
        );
        assert_eq!(
            settings["hooks"]["Stop"][0]["hooks"][0]["command"],
            "heal hook stop"
        );
    }

    #[test]
    fn uninstall_clears_settings_hooks() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        let dest = skills_dest(project);
        install(project, &paths, &dest, false, false).unwrap();
        uninstall(project, &paths, &dest, false).unwrap();
        // settings.json should be deleted entirely (only HEAL hooks were present).
        assert!(!project.join(".claude/settings.json").exists());
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
        assert!(!paths.skills_install_manifest().exists());
    }

    #[test]
    fn uninstall_sweeps_legacy_install_with_no_new_install_present() {
        // Mimic a project still on the pre-`feat(skills)!` layout: extracted
        // plugin tree, marketplace.json, settings.json with the old keys —
        // no new manifest, no new hook block.
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
