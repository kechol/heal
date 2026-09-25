//! `heal skills uninstall` — remove skills an older HEAL copied into the
//! project.
//!
//! Skills ship as the heal Claude Code plugin, so the CLI no longer
//! bundles or extracts them. `uninstall` deletes the directories listed
//! in [`legacy_skills`] and sweeps legacy `.claude/` entries through
//! [`claude_settings::unregister`]. The retired `install`, `update`, and
//! `status` subcommands still parse so scripts from the bundled-skills
//! era fail loudly with the plugin pointer instead of silently doing
//! nothing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::claude_settings;
use crate::cli::SkillsAction;
use crate::legacy_skills;

/// How to get the skills now. Shared with `heal init`'s summary.
pub(crate) const PLUGIN_INSTALL: &str =
    "/plugin marketplace add kechol/heal\n  /plugin install heal@heal";

pub fn run(project: &Path, action: &SkillsAction) -> Result<()> {
    match action {
        SkillsAction::Uninstall { json, .. } => uninstall(project, *json),
        SkillsAction::Install { .. }
        | SkillsAction::Update { .. }
        | SkillsAction::Status { .. } => {
            anyhow::bail!(
                "heal no longer bundles skills; they ship as a Claude Code plugin. In Claude Code, run:\n  \
                 {PLUGIN_INSTALL}\n\
                 Run `heal skills uninstall` to remove skill folders an older heal copied into this project."
            )
        }
    }
}

/// Stable JSON contract for `heal skills uninstall --json`.
#[derive(Debug, Serialize)]
struct UninstallReport {
    /// Project-relative paths removed from disk.
    removed: Vec<String>,
    /// `updated` when `.claude/settings.json` was rewritten or deleted.
    claude_settings: &'static str,
}

fn uninstall(project: &Path, as_json: bool) -> Result<()> {
    let mut removed: Vec<PathBuf> = Vec::new();
    for dir in legacy_skills::find(project) {
        std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
        removed.push(dir);
    }
    // Leave no empty skill roots behind; a root that still holds the
    // user's own skills stays.
    for root in legacy_skills::ROOTS {
        let _ = crate::core::fs::remove_dir_if_empty(&project.join(root));
    }
    let _ = crate::core::fs::remove_dir_if_empty(&project.join(".agents"));

    let settings = claude_settings::unregister(project)?;
    removed.extend(settings.removed);
    let report = UninstallReport {
        removed: removed
            .iter()
            .map(|p| p.strip_prefix(project).unwrap_or(p).display().to_string())
            .collect(),
        claude_settings: if settings.settings_changed {
            "updated"
        } else {
            "unchanged"
        },
    };

    if as_json {
        super::emit_json(&report);
        return Ok(());
    }
    if report.removed.is_empty() {
        println!("Nothing to remove: no skill folders from older heal versions.");
    } else {
        println!("Removed {} path(s):", report.removed.len());
        for path in &report.removed {
            println!("  - {path}");
        }
        println!("Commit the deletion if these folders were tracked in git.");
    }
    if settings.settings_changed {
        println!("Swept legacy heal entries from .claude/settings.json.");
    }
    println!();
    println!("heal skills now ship as a Claude Code plugin. In Claude Code, run:");
    println!("  {PLUGIN_INSTALL}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn uninstall_removes_legacy_skill_dirs_and_keeps_user_skills() {
        let dir = TempDir::new().unwrap();
        let project = dir.path();
        for rel in [
            ".claude/skills/heal-code-patch",
            ".claude/skills/my-own-skill",
            ".agents/skills/heal-setup",
        ] {
            std::fs::create_dir_all(project.join(rel)).unwrap();
            std::fs::write(project.join(rel).join("SKILL.md"), "x").unwrap();
        }

        uninstall(project, true).unwrap();

        assert!(!project.join(".claude/skills/heal-code-patch").exists());
        assert!(project
            .join(".claude/skills/my-own-skill/SKILL.md")
            .exists());
        assert!(
            !project.join(".agents").exists(),
            "empty .agents tree is removed"
        );
    }

    #[test]
    fn uninstall_is_a_noop_on_a_clean_project() {
        let dir = TempDir::new().unwrap();
        uninstall(dir.path(), false).unwrap();
        assert!(!dir.path().join(".claude").exists());
    }

    #[test]
    fn retired_subcommands_fail_with_the_plugin_pointer() {
        let dir = TempDir::new().unwrap();
        let err = run(
            dir.path(),
            &SkillsAction::Install {
                force: false,
                json: false,
                target: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("/plugin install heal@heal"));
    }
}
