//! Skill directories that older HEAL versions extracted into projects.
//!
//! Up to v0.6 the CLI bundled its skills and wrote them to
//! `.claude/skills/heal-*/` (Claude Code) and `.agents/skills/heal-*/`
//! (Codex CLI). Skills now ship as a Claude Code plugin, so these copies
//! are leftovers: `heal doctor` reports them and `heal skills uninstall`
//! removes them. The name list is closed — only directories HEAL itself
//! wrote are ever touched, never a user skill that happens to share the
//! `heal-` prefix.

use std::path::{Path, PathBuf};

/// Every skill name a released HEAL version extracted into a project.
pub const NAMES: &[&str] = &[
    "heal-cli",
    "heal-setup",
    "heal-config",
    "heal-concepts-setup",
    "heal-code-review",
    "heal-code-patch",
    "heal-code-check",
    "heal-code-fix",
    "heal-doc-pair-setup",
    "heal-doc-scaffold",
    "heal-doc-review",
    "heal-doc-patch",
    "heal-test-reporter-setup",
    "heal-test-review",
    "heal-test-patch",
];

/// Project-relative skill roots the bundled install wrote to.
pub const ROOTS: &[&str] = &[".claude/skills", ".agents/skills"];

/// Legacy skill directories present under `project`, in `ROOTS` then
/// `NAMES` order.
#[must_use]
pub fn find(project: &Path) -> Vec<PathBuf> {
    ROOTS
        .iter()
        .flat_map(|root| NAMES.iter().map(move |name| project.join(root).join(name)))
        .filter(|dir| dir.is_dir())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn finds_only_known_names_under_both_roots() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude/skills/heal-code-patch")).unwrap();
        std::fs::create_dir_all(root.join(".agents/skills/heal-setup")).unwrap();
        std::fs::create_dir_all(root.join(".claude/skills/heal-custom")).unwrap();
        std::fs::create_dir_all(root.join(".claude/skills/release")).unwrap();

        let found = find(root);
        assert_eq!(
            found,
            vec![
                root.join(".claude/skills/heal-code-patch"),
                root.join(".agents/skills/heal-setup"),
            ]
        );
    }
}
