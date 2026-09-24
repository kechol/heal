//! The heal plugin ships from this repository (`plugins/heal/`) and is
//! listed by the repository's marketplace (`.claude-plugin/marketplace.json`).
//! Users receive the plugin at the tag the marketplace entry names, and the
//! skills call the CLI released under that same tag, so the versions must
//! move together (`.claude/rules/skills-and-hooks.md` R5).
//!
//! These files live outside the crate, so the checks skip when run from a
//! published crate tarball.

use std::path::{Path, PathBuf};

use serde_json::Value;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    Some(serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}

fn plugin_dir() -> Option<PathBuf> {
    let dir = repo_root().join("plugins/heal");
    dir.is_dir().then_some(dir)
}

#[test]
fn plugin_and_marketplace_versions_match_the_cli() {
    let Some(plugin) = read_json(&repo_root().join("plugins/heal/.claude-plugin/plugin.json"))
    else {
        return;
    };
    let Some(market) = read_json(&repo_root().join(".claude-plugin/marketplace.json")) else {
        panic!("plugins/heal exists but .claude-plugin/marketplace.json does not");
    };
    let cli = env!("CARGO_PKG_VERSION");

    assert_eq!(plugin["name"], "heal");
    assert_eq!(plugin["version"], cli, "plugin.json version");

    let entries = market["plugins"].as_array().expect("marketplace plugins[]");
    let entry = entries
        .iter()
        .find(|p| p["name"] == "heal")
        .expect("marketplace lists the heal plugin");
    assert_eq!(entry["version"], cli, "marketplace entry version");
    assert_eq!(entry["source"]["source"], "git-subdir");
    assert_eq!(entry["source"]["path"], "plugins/heal");
    assert_eq!(
        entry["source"]["ref"],
        format!("v{cli}"),
        "marketplace ref must name the release tag"
    );
}

#[test]
fn session_start_hook_points_at_a_shipped_script() {
    let Some(dir) = plugin_dir() else { return };
    let hooks = read_json(&dir.join("hooks/hooks.json")).expect("hooks/hooks.json");
    let events: Vec<&String> = hooks["hooks"]
        .as_object()
        .expect("hooks object")
        .keys()
        .collect();
    assert_eq!(events, vec!["SessionStart"], "only SessionStart is allowed");
    let command = hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"]
        .as_str()
        .expect("command");
    assert!(command.contains("${CLAUDE_PLUGIN_ROOT}/scripts/session-start.sh"));
    assert!(dir.join("scripts/session-start.sh").is_file());
}

#[test]
fn every_skill_is_named_after_its_directory() {
    let Some(dir) = plugin_dir() else { return };
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir.join("skills")).expect("skills/") {
        let path = entry.unwrap().path();
        let dir_name = path.file_name().unwrap().to_string_lossy().into_owned();
        let body = std::fs::read_to_string(path.join("SKILL.md"))
            .unwrap_or_else(|_| panic!("{dir_name}/SKILL.md"));
        let frontmatter = body
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---"))
            .map_or_else(|| panic!("{dir_name}: missing frontmatter"), |(fm, _)| fm);
        let name = frontmatter
            .lines()
            .find_map(|l| l.strip_prefix("name: "))
            .unwrap_or_else(|| panic!("{dir_name}: missing name"));
        assert_eq!(name, dir_name, "SKILL.md name must equal its directory");
        assert!(
            frontmatter.contains(&format!("/heal:{dir_name}")),
            "{dir_name}: description should end with the /heal:{dir_name} trigger"
        );
        names.push(dir_name);
    }
    names.sort();
    assert_eq!(names, ["docs", "refactor", "setup", "tests"]);
}
