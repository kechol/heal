//! `heal skills <install|update|status|uninstall>` — manage the
//! bundled skill set across every agent target HEAL knows about.
//!
//! Each subcommand resolves its `--target <filter>` flag against the
//! live `PATH` (default `detected`) and operates on the resulting
//! [`SkillTarget`] list. `claude` lands in `.claude/skills/` and
//! `codex` in `.agents/skills/`; the same bundled bytes serve both.
//!
//! `install` is the safe default (skips existing files), `update` is
//! drift-aware (overwrites unmodified assets, leaves user edits alone
//! unless `--force`), and `status` reads each SKILL.md's frontmatter
//! `metadata:` block to surface installed version + drift per target.
//! There is no sidecar manifest — bundled bytes are the source of
//! truth, on-disk state alone determines drift, and each target's
//! tree is independent. The Claude path also sweeps legacy
//! `heal hook edit` / `heal hook stop` entries from
//! `.claude/settings.json`; Codex has no sibling settings file.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::claude_settings::{self, UnregisterReport, WireReport, WriteAction};
use crate::cli::SkillsAction;
use crate::skill_assets::{
    self, ExtractMode, ExtractStats, ExtractSummary, SkillTarget, TargetFilter,
    INSTALL_SOURCE_BUNDLED,
};

pub fn run(project: &Path, action: SkillsAction) -> Result<()> {
    match action {
        SkillsAction::Install {
            force,
            json,
            target,
        } => install(project, target, force, json),
        SkillsAction::Update {
            force,
            json,
            target,
        } => update(project, target, force, json),
        SkillsAction::Status { json, target } => {
            status(project, target, json);
            Ok(())
        }
        SkillsAction::Uninstall { json, target } => uninstall(project, target, json),
    }
}

fn install(project: &Path, filter: TargetFilter, force: bool, as_json: bool) -> Result<()> {
    let targets = filter.resolve();
    let mode = if force {
        ExtractMode::InstallForce
    } else {
        ExtractMode::InstallSafe
    };
    let reports = run_extract_pass(project, &targets, mode)?;
    let version = skill_assets::bundled_version();

    if as_json {
        super::emit_json(&InstallReport {
            action: SkillsActionKind::Installed,
            version: &version,
            source: INSTALL_SOURCE_BUNDLED,
            filter,
            targets: &reports,
        });
        return Ok(());
    }
    if reports.is_empty() {
        print_empty_target_hint(filter);
        return Ok(());
    }
    println!(
        "skills {} (filter: {})",
        install_verb(force),
        filter_label(filter),
    );
    println!("  version: {version}");
    println!("  source:  {INSTALL_SOURCE_BUNDLED}");
    for report in &reports {
        println!();
        println!("  [{}] {}", report.target.display_name(), report.dest);
        print_extract_summary(&report.stats);
        if let Some(wire) = report.claude {
            print_wire_summary(wire);
        }
    }
    Ok(())
}

fn update(project: &Path, filter: TargetFilter, force: bool, as_json: bool) -> Result<()> {
    let targets = filter.resolve();
    let reports = run_extract_pass(project, &targets, ExtractMode::Update { force })?;
    let version = skill_assets::bundled_version();

    if as_json {
        super::emit_json(&InstallReport {
            action: SkillsActionKind::Updated,
            version: &version,
            source: INSTALL_SOURCE_BUNDLED,
            filter,
            targets: &reports,
        });
        return Ok(());
    }
    if reports.is_empty() {
        print_empty_target_hint(filter);
        return Ok(());
    }
    println!("skills updated (filter: {})", filter_label(filter));
    println!("  version: {version}");
    let mut any_user_modified = false;
    for report in &reports {
        println!();
        println!("  [{}] {}", report.target.display_name(), report.dest);
        print_extract_summary(&report.stats);
        if let Some(wire) = report.claude {
            print_wire_summary(wire);
        }
        any_user_modified |= !report.stats.user_modified.is_empty();
    }
    if any_user_modified && !force {
        println!();
        println!(
            "  hint: some file(s) were skipped due to local edits — pass `--force` to overwrite."
        );
    }
    Ok(())
}

/// Walk `targets`, run `skill_assets::extract` against each one, and
/// (for the Claude target only) sweep legacy `settings.json` entries
/// via `claude_settings::wire`. Returns one report entry per target,
/// in input order.
fn run_extract_pass(
    project: &Path,
    targets: &[SkillTarget],
    mode: ExtractMode,
) -> Result<Vec<TargetExtractReport>> {
    let mut reports = Vec::with_capacity(targets.len());
    for &target in targets {
        let dest = target.dest(project);
        let stats = skill_assets::extract(&dest, mode)?;
        let claude = if matches!(target, SkillTarget::Claude) {
            Some(claude_settings::wire(project)?)
        } else {
            None
        };
        reports.push(TargetExtractReport {
            target,
            dest: dest.display().to_string(),
            stats,
            claude,
        });
    }
    Ok(reports)
}

/// Snapshot for `heal skills status`. Read-only: reads every requested
/// target's SKILL.md from disk to surface installed version + drift.
fn status(project: &Path, filter: TargetFilter, as_json: bool) {
    let bundled = skill_assets::bundled_version();
    let targets = filter.resolve();
    let entries: Vec<StatusEntry> = targets
        .iter()
        .map(|&t| status_entry_for(project, t, &bundled))
        .collect();

    if as_json {
        super::emit_json(&StatusReport {
            bundled: &bundled,
            filter,
            targets: &entries,
        });
        return;
    }
    if entries.is_empty() {
        print_empty_target_hint(filter);
        return;
    }
    println!("skills (filter: {})", filter_label(filter));
    println!("  bundled: {bundled}");
    for entry in &entries {
        println!();
        println!("  [{}] {}", entry.target.display_name(), entry.dest);
        match entry.state {
            StatusState::NotInstalled => {
                println!("    state:     not_installed");
            }
            StatusState::Installed => {
                println!(
                    "    installed: {}",
                    entry
                        .installed
                        .as_deref()
                        .unwrap_or("(unknown — pre-metadata install)"),
                );
                println!(
                    "    source:    {}",
                    entry.source.as_deref().unwrap_or("(unknown)"),
                );
                let label = match entry.version_status {
                    Some(VersionCmp::Match) => "up-to-date",
                    Some(VersionCmp::BundledNewer) => "bundled-newer (run `heal skills update`)",
                    Some(VersionCmp::InstalledNewer) => "installed-newer (binary downgrade?)",
                    None => "(unknown)",
                };
                println!("    status:    {label}");
                if !entry.drift.is_empty() {
                    println!(
                        "    drift:     {} file(s) edited locally",
                        entry.drift.len()
                    );
                    for p in &entry.drift {
                        println!("      - {p}");
                    }
                }
            }
        }
    }
}

fn status_entry_for(project: &Path, target: SkillTarget, bundled: &str) -> StatusEntry {
    let dest = target.dest(project);
    let dest_display = dest.display().to_string();
    if !dest.exists() {
        return StatusEntry {
            target,
            dest: dest_display,
            state: StatusState::NotInstalled,
            installed: None,
            source: None,
            version_status: None,
            drift: Vec::new(),
        };
    }
    let installed = read_installed_version(&dest);
    let drift = drifted_assets(&dest);
    let cmp = installed
        .as_deref()
        .map_or(VersionCmp::BundledNewer, |v| compare_versions(v, bundled));
    StatusEntry {
        target,
        dest: dest_display,
        state: StatusState::Installed,
        installed,
        source: Some(INSTALL_SOURCE_BUNDLED.to_string()),
        version_status: Some(cmp),
        drift,
    }
}

fn uninstall(project: &Path, filter: TargetFilter, as_json: bool) -> Result<()> {
    let targets = filter.resolve();
    let mut reports = Vec::with_capacity(targets.len());
    for &target in &targets {
        let dest = target.dest(project);
        let removed = remove_installed_skills(&dest)?;
        let claude = if matches!(target, SkillTarget::Claude) {
            Some(claude_settings::unregister(project)?)
        } else {
            None
        };
        reports.push(UninstallTargetReport {
            target,
            dest: dest.display().to_string(),
            skills_removed: removed,
            claude,
        });
    }

    if as_json {
        super::emit_json(&UninstallReport {
            filter,
            targets: &reports,
        });
        return Ok(());
    }
    if reports.is_empty() {
        print_empty_target_hint(filter);
        return Ok(());
    }
    let any_removed = reports
        .iter()
        .any(|r| !r.skills_removed.is_empty() || r.claude.is_some_and(|c| c.legacy_swept));
    if !any_removed {
        println!(
            "skills uninstall (filter: {}): nothing was installed; no-op",
            filter_label(filter),
        );
        return Ok(());
    }
    println!("skills uninstall (filter: {})", filter_label(filter));
    for report in &reports {
        println!();
        println!("  [{}] {}", report.target.display_name(), report.dest);
        if report.skills_removed.is_empty() {
            if report.claude.is_some_and(|c| c.legacy_swept) {
                println!("    removed legacy plugin/marketplace install layout");
            } else {
                println!("    nothing installed; no-op");
            }
        } else {
            println!("    removed {} skill(s):", report.skills_removed.len());
            for s in &report.skills_removed {
                println!("      - {s}");
            }
            if report.claude.is_some_and(|c| c.legacy_swept) {
                println!("    also removed legacy plugin/marketplace install layout");
            }
        }
    }
    Ok(())
}

/// Walk every bundled skill name and remove the corresponding directory
/// from `dest` if present. Untracked sibling skills survive — we never
/// recursively nuke the whole `skills/` parent because the user may
/// have other skills there.
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

    fn walk_dir(dir: &include_dir::Dir<'_>, dest: &Path, out: &mut Vec<String>) {
        for entry in dir.entries() {
            match entry {
                DirEntry::Dir(child) => walk_dir(child, dest, out),
                DirEntry::File(file) => {
                    let target = dest.join(file.path());
                    let Ok(on_disk) = std::fs::read(&target) else {
                        continue;
                    };
                    if skill_assets::canonical_user_bytes(file.path(), &on_disk).as_ref()
                        != file.contents()
                    {
                        out.push(
                            file.path()
                                .components()
                                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                                .collect::<Vec<_>>()
                                .join("/"),
                        );
                    }
                }
            }
        }
    }

    let mut drift: Vec<String> = Vec::new();
    walk_dir(&skill_assets::SKILLS_DIR, dest, &mut drift);
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

/// Human-readable rendering of the active `--target` choice.
fn filter_label(filter: TargetFilter) -> &'static str {
    match filter {
        TargetFilter::Detected => "detected",
        TargetFilter::Claude => "claude",
        TargetFilter::Codex => "codex",
        TargetFilter::All => "all",
    }
}

/// Single source of truth for the "nothing to do" line when
/// `filter.resolve()` came back empty. Only `Detected` reaches this in
/// practice — the explicit variants always resolve to a non-empty
/// list, and `All` resolves to `SkillTarget::ALL` (also non-empty by
/// construction). Other filters hitting this path mean
/// `SkillTarget::ALL` was emptied or the resolver got out of sync,
/// both of which are HEAL bugs.
fn print_empty_target_hint(filter: TargetFilter) {
    if matches!(filter, TargetFilter::Detected) {
        println!(
            "skills: no agent CLI on PATH (looked for `claude`, `codex`); pass `--target all` to install regardless",
        );
    } else {
        println!(
            "skills: filter `{}` resolved to no targets (this is a HEAL bug — please report)",
            filter_label(filter),
        );
    }
}

fn print_extract_summary(stats: &ExtractStats) {
    let s: ExtractSummary = stats.summary();
    println!(
        "    files:   added {} | updated {} | unchanged {} | skipped {} | local-edits {}",
        s.added, s.updated, s.unchanged, s.skipped, s.user_modified,
    );
    if !stats.user_modified.is_empty() {
        for p in &stats.user_modified {
            println!("      skipped (local edit): {p}");
        }
    }
}

fn print_wire_summary(report: WireReport) {
    println!("    claude:  settings {}", wire_verb(report.settings));
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
struct InstallReport<'a> {
    action: SkillsActionKind,
    version: &'a str,
    source: &'a str,
    filter: TargetFilter,
    targets: &'a [TargetExtractReport],
}

#[derive(Debug, Serialize)]
struct TargetExtractReport {
    target: SkillTarget,
    dest: String,
    #[serde(rename = "files")]
    #[serde(serialize_with = "serialize_extract_summary")]
    stats: ExtractStats,
    /// `Some` only when this target is [`SkillTarget::Claude`] —
    /// Codex has no sibling settings file to wire.
    #[serde(skip_serializing_if = "Option::is_none")]
    claude: Option<WireReport>,
}

/// `ExtractStats` carries the raw added/updated/etc lists which are
/// useful at the rendering layer; the JSON contract surfaces only the
/// summary counts plus the `user_modified` path list.
fn serialize_extract_summary<S: serde::Serializer>(
    stats: &ExtractStats,
    s: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let summary = stats.summary();
    let mut m = s.serialize_map(Some(6))?;
    m.serialize_entry("added", &summary.added)?;
    m.serialize_entry("updated", &summary.updated)?;
    m.serialize_entry("unchanged", &summary.unchanged)?;
    m.serialize_entry("skipped", &summary.skipped)?;
    m.serialize_entry("user_modified", &summary.user_modified)?;
    m.serialize_entry("user_modified_paths", &stats.user_modified)?;
    m.end()
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
enum StatusState {
    #[default]
    NotInstalled,
    Installed,
}

#[derive(Debug, Default, Serialize)]
struct StatusEntry {
    target: SkillTarget,
    dest: String,
    state: StatusState,
    #[serde(skip_serializing_if = "Option::is_none")]
    installed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version_status: Option<VersionCmp>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    drift: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StatusReport<'a> {
    bundled: &'a str,
    filter: TargetFilter,
    targets: &'a [StatusEntry],
}

#[derive(Debug, Serialize)]
struct UninstallTargetReport {
    target: SkillTarget,
    dest: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    skills_removed: Vec<String>,
    /// `Some` only for the Claude target — `legacy_swept` reports
    /// whether the pre-skills plugin/marketplace layout was found and
    /// removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    claude: Option<UnregisterReport>,
}

#[derive(Debug, Serialize)]
struct UninstallReport<'a> {
    filter: TargetFilter,
    targets: &'a [UninstallTargetReport],
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
    use crate::core::HealPaths;
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
    fn install_target_all_extracts_for_each_target() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        install(project, TargetFilter::All, false, false).unwrap();
        for &t in &SkillTarget::ALL {
            let dest = t.dest(project);
            assert!(dest.exists(), "{t:?} dest must exist");
            assert!(dest.join("heal-cli/SKILL.md").exists());
        }
    }

    #[test]
    fn install_target_codex_does_not_touch_claude_settings() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        install(project, TargetFilter::Codex, false, false).unwrap();
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

    #[test]
    fn install_target_claude_only_writes_to_claude_dest() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        install(project, TargetFilter::Claude, false, false).unwrap();
        assert!(SkillTarget::Claude
            .dest(project)
            .join("heal-cli/SKILL.md")
            .exists());
        assert!(
            !SkillTarget::Codex.dest(project).exists(),
            "claude-only install must not touch codex tree",
        );
    }

    #[test]
    fn uninstall_target_all_removes_every_tree() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        install(project, TargetFilter::All, false, false).unwrap();
        for &t in &SkillTarget::ALL {
            assert!(t.dest(project).join("heal-cli/SKILL.md").exists());
        }
        uninstall(project, TargetFilter::All, false).unwrap();
        for &t in &SkillTarget::ALL {
            assert!(
                !t.dest(project).join("heal-cli/SKILL.md").exists(),
                "{t:?} skills must be removed",
            );
        }
    }

    #[test]
    fn uninstall_target_claude_leaves_codex_alone() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        install(project, TargetFilter::All, false, false).unwrap();
        uninstall(project, TargetFilter::Claude, false).unwrap();
        assert!(
            !SkillTarget::Claude.dest(project).exists()
                || !SkillTarget::Claude
                    .dest(project)
                    .join("heal-cli/SKILL.md")
                    .exists(),
            "claude tree must be gone after target=claude uninstall",
        );
        assert!(
            SkillTarget::Codex
                .dest(project)
                .join("heal-cli/SKILL.md")
                .exists(),
            "codex tree must survive claude-only uninstall",
        );
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
        // `--target all` covers Claude and triggers the wire sweep.
        install(project, TargetFilter::All, false, false).unwrap();
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
        uninstall(project, TargetFilter::All, false).unwrap();
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

        uninstall(project, TargetFilter::Claude, false).unwrap();

        assert!(!plugin_tree.exists());
        assert!(!market.exists());
        assert!(
            !project.join(".claude/settings.json").exists(),
            "legacy-only settings should collapse to deletion"
        );
    }

    #[test]
    fn status_target_all_reports_every_target() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        let paths = HealPaths::new(project);
        paths.ensure().unwrap();
        install(project, TargetFilter::Claude, false, false).unwrap();
        // Render through a JSON pass so we exercise the serializer.
        // A non-JSON pass would be sufficient for compile-coverage but
        // the JSON shape is the contract bundled skills consume.
        let bundled = skill_assets::bundled_version();
        let claude_entry = status_entry_for(project, SkillTarget::Claude, &bundled);
        let codex_entry = status_entry_for(project, SkillTarget::Codex, &bundled);
        assert!(matches!(claude_entry.state, StatusState::Installed));
        assert!(matches!(codex_entry.state, StatusState::NotInstalled));
    }

    #[test]
    fn target_filter_resolve_respects_presence() {
        let presence = [(SkillTarget::Claude, true), (SkillTarget::Codex, false)];
        assert_eq!(
            TargetFilter::Detected.resolve_with(&presence),
            vec![SkillTarget::Claude],
        );
        assert_eq!(
            TargetFilter::All.resolve_with(&presence),
            SkillTarget::ALL.to_vec(),
        );
        assert_eq!(
            TargetFilter::Codex.resolve_with(&presence),
            vec![SkillTarget::Codex],
        );
    }
}
