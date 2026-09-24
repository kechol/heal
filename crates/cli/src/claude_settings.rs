//! Clean up what older HEAL versions wrote into `.claude/`.
//!
//! Skills now ship as a Claude Code plugin, and HEAL registers no
//! Claude Code hooks, so nothing here writes new configuration. `heal
//! skills uninstall` calls [`unregister`] to sweep:
//!
//!   - legacy `heal hook edit` / `heal hook stop` entries from
//!     `.claude/settings.json`, and
//!   - the pre-v0.2 local marketplace layout (`.claude/plugins/heal/`,
//!     a `heal-local` `.claude-plugin/marketplace.json`, and its
//!     `extraKnownMarketplaces` / `enabledPlugins` keys).
//!
//! Settings outside the swept entries are preserved via a
//! `serde_json::Value` round-trip. A `.claude-plugin/marketplace.json`
//! that is not HEAL's `heal-local` one — a project that is itself a
//! plugin marketplace — is never touched.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

const SETTINGS_FILE: &str = ".claude/settings.json";

/// Older heal versions wired up a marketplace + plugin tree under
/// these paths and registered a `heal-local` marketplace entry plus a
/// `heal@heal-local` enabled-plugin flag in `settings.json`. Modern
/// installs never write any of them, but uninstall sweeps them so users
/// upgrading don't end up with double hook firings or a stale plugin
/// tree on disk.
const LEGACY_MARKETPLACE_FILE: &str = ".claude-plugin/marketplace.json";
const LEGACY_MARKETPLACE_DIR: &str = ".claude-plugin";
const LEGACY_PLUGIN_DEST_REL: &str = ".claude/plugins/heal";
const LEGACY_MARKETPLACE_NAME: &str = "heal-local";
const LEGACY_ENABLED_PLUGIN_KEY: &str = "heal@heal-local";

/// Legacy hook commands swept out of `settings.json` by `register` /
/// `unregister`. HEAL doesn't add these anymore — the strings live on
/// so upgrades from versions that did install them stay clean.
const LEGACY_HEAL_COMMANDS: &[&str] = &["heal hook edit", "heal hook stop"];

/// Outcome of [`unregister`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnregisterReport {
    /// Legacy plugin / marketplace paths removed from disk.
    pub removed: Vec<PathBuf>,
    /// `.claude/settings.json` was rewritten or deleted.
    pub settings_changed: bool,
}

/// Remove HEAL's hook entries from `settings.json`, plus any legacy
/// marketplace / `enabledPlugins` keys and the old plugin/marketplace
/// files on disk. User entries (anything whose `command` doesn't match
/// a HEAL hook) survive untouched. The file itself is removed when
/// nothing else remains.
pub fn unregister(project: &Path) -> Result<UnregisterReport> {
    let removed = remove_legacy_artifacts(project)?;
    let settings_path = project.join(SETTINGS_FILE);
    let Ok(prior) = std::fs::read_to_string(&settings_path) else {
        return Ok(UnregisterReport {
            removed,
            settings_changed: false,
        });
    };
    let mut value: Value = serde_json::from_str(&prior)
        .with_context(|| format!("parsing {}", settings_path.display()))?;
    remove_heal_hooks(&mut value);
    remove_legacy_settings_keys(&mut value);
    if value.as_object().is_some_and(serde_json::Map::is_empty) {
        std::fs::remove_file(&settings_path)
            .with_context(|| format!("removing {}", settings_path.display()))?;
        return Ok(UnregisterReport {
            removed,
            settings_changed: true,
        });
    }
    let cleaned = format!(
        "{}\n",
        serde_json::to_string_pretty(&value).expect("settings serialization is infallible")
    );
    let settings_changed = cleaned != prior;
    if settings_changed {
        crate::core::fs::atomic_write(&settings_path, cleaned.as_bytes())
            .with_context(|| format!("writing {}", settings_path.display()))?;
    }
    Ok(UnregisterReport {
        removed,
        settings_changed,
    })
}

/// Sweep on-disk artifacts left over from the old local-marketplace
/// install layout and return the paths removed. Missing paths are
/// no-ops via `ErrorKind::NotFound` rather than a racy `exists()` check.
/// The marketplace file goes only when it is HEAL's `heal-local` one.
fn remove_legacy_artifacts(project: &Path) -> Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    let plugin_tree = project.join(LEGACY_PLUGIN_DEST_REL);
    if remove_dir_all_if_present(&plugin_tree)? {
        removed.push(plugin_tree);
    }
    let market = project.join(LEGACY_MARKETPLACE_FILE);
    if is_legacy_marketplace(&market) && remove_file_if_present(&market)? {
        removed.push(market);
        // Best-effort: leave the dir if anything else lives in it.
        let _ = crate::core::fs::remove_dir_if_empty(&project.join(LEGACY_MARKETPLACE_DIR));
    }
    Ok(removed)
}

/// True when `path` parses as a marketplace named `heal-local` — the
/// only marketplace file HEAL ever wrote. Anything else (missing,
/// unparseable, another name) is not ours.
fn is_legacy_marketplace(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .is_some_and(|v| v.get("name").and_then(Value::as_str) == Some(LEGACY_MARKETPLACE_NAME))
}

fn remove_dir_all_if_present(path: &Path) -> Result<bool> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
    }
}

fn remove_file_if_present(path: &Path) -> Result<bool> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
    }
}

/// Strip the legacy `extraKnownMarketplaces["heal-local"]` and
/// `enabledPlugins["heal@heal-local"]` entries from a settings.json
/// value. Empty parent objects are dropped after removal.
fn remove_legacy_settings_keys(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    if let Some(market) = obj
        .get_mut("extraKnownMarketplaces")
        .and_then(Value::as_object_mut)
    {
        market.remove(LEGACY_MARKETPLACE_NAME);
        if market.is_empty() {
            obj.remove("extraKnownMarketplaces");
        }
    }
    if let Some(enabled) = obj.get_mut("enabledPlugins").and_then(Value::as_object_mut) {
        enabled.remove(LEGACY_ENABLED_PLUGIN_KEY);
        if enabled.is_empty() {
            obj.remove("enabledPlugins");
        }
    }
}

/// Walk every block under every event and drop inner-hook entries whose
/// `command` matches a [`LEGACY_HEAL_COMMANDS`] entry. Empty inner-hook
/// arrays remove the containing block; empty event arrays are dropped
/// from `hooks`; an empty `hooks` object is dropped from the root.
fn remove_heal_hooks(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    let Some(hooks) = obj.get_mut("hooks").and_then(Value::as_object_mut) else {
        return;
    };

    for blocks in hooks.values_mut() {
        let Some(blocks) = blocks.as_array_mut() else {
            continue;
        };
        for block in blocks.iter_mut() {
            if let Some(inner) = block.get_mut("hooks").and_then(Value::as_array_mut) {
                inner.retain(|h| {
                    h.get("command")
                        .and_then(Value::as_str)
                        .is_none_or(|c| !LEGACY_HEAL_COMMANDS.contains(&c))
                });
            }
        }
        blocks.retain(|block| {
            block
                .get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|inner| !inner.is_empty())
        });
    }

    hooks.retain(|_, blocks| !blocks.as_array().is_some_and(Vec::is_empty));
    if hooks.is_empty() {
        obj.remove("hooks");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn unregister_sweeps_legacy_heal_hook_commands() {
        let dir = TempDir::new().unwrap();
        let settings_path = dir.path().join(SETTINGS_FILE);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        // Pre-v0.3 install: HEAL's edit/stop hooks plus a user hook.
        std::fs::write(
            &settings_path,
            r#"{
              "theme": "dark",
              "hooks": {
                "PostToolUse": [
                  { "matcher": "Edit|Write|MultiEdit",
                    "hooks": [
                      { "type": "command", "command": "heal hook edit" },
                      { "type": "command", "command": "echo edit" }
                    ]
                  }
                ],
                "Stop": [
                  { "hooks": [{ "type": "command", "command": "heal hook stop" }] }
                ]
              }
            }"#,
        )
        .unwrap();
        let report = unregister(dir.path()).unwrap();
        assert!(report.settings_changed);
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        let post = v["hooks"]["PostToolUse"][0]["hooks"].as_array().unwrap();
        let cmds: Vec<&str> = post
            .iter()
            .map(|h| h["command"].as_str().unwrap())
            .collect();
        assert_eq!(cmds, vec!["echo edit"]);
        // Stop block had no user entry — collapses out.
        assert!(v["hooks"].get("Stop").is_none());
    }

    #[test]
    fn unregister_is_idempotent() {
        let dir = TempDir::new().unwrap();
        assert_eq!(unregister(dir.path()).unwrap(), UnregisterReport::default());
        assert_eq!(unregister(dir.path()).unwrap(), UnregisterReport::default());
    }

    #[test]
    fn unregister_strips_only_legacy_heal_entries() {
        let dir = TempDir::new().unwrap();
        let settings_path = dir.path().join(SETTINGS_FILE);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{
              "theme": "dark",
              "hooks": {
                "Stop": [
                  { "hooks": [
                    { "type": "command", "command": "heal hook stop" },
                    { "type": "command", "command": "echo bye" }
                  ]}
                ]
              }
            }"#,
        )
        .unwrap();
        unregister(dir.path()).unwrap();
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        let stop_cmds: Vec<&str> = v["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|b| b["hooks"].as_array().unwrap())
            .map(|h| h["command"].as_str().unwrap())
            .collect();
        assert_eq!(stop_cmds, vec!["echo bye"]);
    }

    #[test]
    fn unregister_when_nothing_was_wired_is_noop() {
        let dir = TempDir::new().unwrap();
        unregister(dir.path()).unwrap();
        assert!(!dir.path().join(SETTINGS_FILE).exists());
    }

    #[test]
    fn unregister_sweeps_legacy_marketplace_and_plugin_tree() {
        let dir = TempDir::new().unwrap();
        // Stage a pre-`feat(skills)!` install layout.
        let plugin_tree = dir.path().join(LEGACY_PLUGIN_DEST_REL);
        std::fs::create_dir_all(&plugin_tree).unwrap();
        std::fs::write(plugin_tree.join("plugin.json"), "{}").unwrap();
        let market = dir.path().join(LEGACY_MARKETPLACE_FILE);
        std::fs::create_dir_all(market.parent().unwrap()).unwrap();
        std::fs::write(&market, r#"{"name":"heal-local","plugins":[]}"#).unwrap();

        let report = unregister(dir.path()).unwrap();
        assert_eq!(report.removed, vec![plugin_tree.clone(), market.clone()]);
        assert!(!plugin_tree.exists(), "legacy plugin tree must be removed");
        assert!(!market.exists(), "legacy marketplace.json must be removed");
        assert!(
            !dir.path().join(LEGACY_MARKETPLACE_DIR).exists(),
            "empty legacy marketplace dir must be removed"
        );
    }

    #[test]
    fn unregister_sweeps_legacy_settings_keys() {
        let dir = TempDir::new().unwrap();
        let settings_path = dir.path().join(SETTINGS_FILE);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{
              "theme": "dark",
              "enabledPlugins": { "heal@heal-local": true, "other@x": true },
              "extraKnownMarketplaces": {
                "heal-local": { "source": { "source": "file", "path": "./.claude-plugin/marketplace.json" } }
              }
            }"#,
        )
        .unwrap();
        unregister(dir.path()).unwrap();
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert!(v["enabledPlugins"].get("heal@heal-local").is_none());
        assert_eq!(v["enabledPlugins"]["other@x"], true);
        assert!(
            v.get("extraKnownMarketplaces").is_none(),
            "legacy-only marketplaces map must be dropped"
        );
    }

    #[test]
    fn unregister_legacy_only_install_collapses_settings() {
        // A pre-`feat(skills)!` install that never had the new hooks
        // section. After unregister the file should be gone entirely.
        let dir = TempDir::new().unwrap();
        let settings_path = dir.path().join(SETTINGS_FILE);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{
              "enabledPlugins": { "heal@heal-local": true },
              "extraKnownMarketplaces": {
                "heal-local": { "source": { "source": "file", "path": "./.claude-plugin/marketplace.json" } }
              }
            }"#,
        )
        .unwrap();
        unregister(dir.path()).unwrap();
        assert!(!settings_path.exists());
    }

    #[test]
    fn unregister_keeps_a_marketplace_that_is_not_heal_local() {
        // A project that is itself a plugin marketplace (this repository
        // is one) must keep its manifest.
        let dir = TempDir::new().unwrap();
        let market = dir.path().join(LEGACY_MARKETPLACE_FILE);
        std::fs::create_dir_all(market.parent().unwrap()).unwrap();
        std::fs::write(&market, r#"{"name":"heal","plugins":[]}"#).unwrap();
        let report = unregister(dir.path()).unwrap();
        assert!(report.removed.is_empty());
        assert!(market.exists());
    }
}
