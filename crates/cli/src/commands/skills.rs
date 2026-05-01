//! `heal skills <install|update|status|uninstall>` — manage the bundled
//! Claude plugin under `.claude/plugins/heal/`.
//!
//! `install` is the safe default (skips existing files), `update` is
//! drift-aware (overwrites unchanged-since-install assets, leaves user
//! edits alone unless `--force`), and `status` exposes the bundled vs.
//! installed version plus any drift surfaced by the manifest.

use std::path::Path;

use anyhow::{Context, Result};

use crate::claude_settings::{self, WireReport, WriteAction};
use crate::cli::SkillsAction;
use crate::plugin_assets::{
    self, plugin_dest, ExtractMode, ExtractStats, ExtractSummary, InstallManifest, INSTALL_MANIFEST,
};

pub fn run(project: &Path, action: SkillsAction) -> Result<()> {
    let dest = plugin_dest(project);
    match action {
        SkillsAction::Install { force } => install(project, &dest, force),
        SkillsAction::Update { force } => update(project, &dest, force),
        SkillsAction::Status => {
            status(&dest);
            Ok(())
        }
        SkillsAction::Uninstall => uninstall(project, &dest),
    }
}

fn install(project: &Path, dest: &Path, force: bool) -> Result<()> {
    let mode = if force {
        ExtractMode::InstallForce
    } else {
        ExtractMode::InstallSafe
    };
    let (stats, manifest) = plugin_assets::extract(dest, mode)?;
    let wire = claude_settings::wire(project, &manifest.heal_version)?;
    println!("plugin {} at {}", install_verb(force), dest.display());
    println!("  version: {}", manifest.heal_version);
    println!("  source:  {}", manifest.source);
    print_extract_summary(&stats);
    print_wire_summary(wire);
    Ok(())
}

fn update(project: &Path, dest: &Path, force: bool) -> Result<()> {
    let (stats, manifest) = plugin_assets::extract(dest, ExtractMode::Update { force })?;
    let wire = claude_settings::wire(project, &manifest.heal_version)?;
    println!("plugin updated at {}", dest.display());
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
///
/// Outputs one of four cases — not installed; installed but no manifest
/// (legacy / hand-extracted tree); installed and up-to-date; installed
/// and bundled-newer (suggests `heal skills update`) — each followed by
/// a drift list when on-disk fingerprints diverge from the manifest.
fn status(dest: &Path) {
    if !dest.exists() {
        println!("plugin: not installed (run `heal skills install`)");
        return;
    }
    let bundled = plugin_assets::bundled_version().unwrap_or_else(|| "unknown".into());
    let manifest_path = dest.join(INSTALL_MANIFEST);
    let Some(manifest) = InstallManifest::load(dest) else {
        println!(
            "plugin: directory exists at {} but no manifest ({})",
            dest.display(),
            manifest_path.display()
        );
        println!("  bundled: {bundled}");
        println!("  hint:    run `heal skills update --force` to refresh metadata.");
        return;
    };
    println!("plugin: installed at {}", dest.display());
    println!("  installed: {}", manifest.heal_version);
    println!("  bundled:   {bundled}");
    println!("  installed at: {}", manifest.installed_at.to_rfc3339());
    println!("  source:    {}", manifest.source);

    let cmp = compare_versions(&manifest.heal_version, &bundled);
    let label = match cmp {
        VersionCmp::Match => "up-to-date",
        VersionCmp::BundledNewer => "bundled-newer (run `heal skills update`)",
        VersionCmp::InstalledNewer => "installed-newer (binary downgrade?)",
    };
    println!("  status:    {label}");

    let drift = drifted_assets(dest, &manifest);
    if drift.is_empty() {
        return;
    }
    println!("  drift:     {} file(s) edited locally", drift.len());
    for p in drift {
        println!("    - {p}");
    }
}

fn uninstall(project: &Path, dest: &Path) -> Result<()> {
    let plugin_existed = dest.exists();
    if plugin_existed {
        std::fs::remove_dir_all(dest).with_context(|| format!("removing {}", dest.display()))?;
        println!("removed {}", dest.display());
    } else {
        println!("plugin not installed; nothing to do");
    }
    claude_settings::unregister(project)?;
    Ok(())
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
    println!(
        "  claude:  marketplace {} | settings {}",
        wire_verb(report.marketplace),
        wire_verb(report.settings),
    );
}

fn wire_verb(action: WriteAction) -> &'static str {
    match action {
        WriteAction::Created => "created",
        WriteAction::Updated => "updated",
        WriteAction::Unchanged => "unchanged",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VersionCmp {
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
    let mut drift = Vec::new();
    for (rel, fp) in &manifest.assets {
        let p = dest.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        if &plugin_assets::fingerprint(&bytes) != fp {
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
        let dest = dir.path();
        let (_, manifest) = plugin_assets::extract(dest, ExtractMode::InstallSafe).unwrap();
        // No drift right after install.
        assert!(drifted_assets(dest, &manifest).is_empty());

        std::fs::write(dest.join("hooks/claude-stop.sh"), "tampered\n").unwrap();
        let drift = drifted_assets(dest, &manifest);
        assert_eq!(drift, vec!["hooks/claude-stop.sh".to_string()]);
    }

    #[test]
    fn uninstall_removes_plugin_dir() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let dest = project.join(".claude/plugins/heal");
        plugin_assets::extract(&dest, ExtractMode::InstallSafe).unwrap();
        uninstall(project, &dest).unwrap();
        assert!(!dest.exists());
    }

    #[test]
    fn install_wires_claude_marketplace_and_settings() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let dest = project.join(".claude/plugins/heal");
        install(project, &dest, false).unwrap();
        assert!(project.join(".claude-plugin/marketplace.json").exists());
        let settings = std::fs::read_to_string(project.join(".claude/settings.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&settings).unwrap();
        assert_eq!(v["enabledPlugins"]["heal@heal-local"], true);
        assert_eq!(
            v["extraKnownMarketplaces"]["heal-local"]["source"]["source"],
            "file"
        );
    }

    #[test]
    fn uninstall_clears_marketplace_and_settings() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let dest = project.join(".claude/plugins/heal");
        install(project, &dest, false).unwrap();
        uninstall(project, &dest).unwrap();
        assert!(!dest.exists());
        assert!(!project.join(".claude-plugin/marketplace.json").exists());
        assert!(!project.join(".claude/settings.json").exists());
    }
}
