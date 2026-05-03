//! Manage HEAL's `<project>/.claude/settings.json` registration.
//!
//! Skills are extracted directly under `.claude/skills/` (handled by
//! [`crate::skill_assets`]); Claude Code discovers them natively without
//! a marketplace. This module owns the *other* half of the wiring.
//!
//! HEAL does not register any Claude Code hooks. `wire` / `register`
//! exist so `heal init` / `heal skills install` can:
//!
//!   - sweep legacy `heal hook edit` / `heal hook stop` entries left
//!     over from earlier installs, and
//!   - clean up the pre-v0.2 marketplace plugin tree if present.
//!
//! Settings outside the swept entries are preserved byte-for-byte via a
//! `serde_json::Value` round-trip.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;
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

/// Outcome reported by [`register`]. Drives the CLI status line and
/// lets callers report no-op vs. mutation distinctly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteAction {
    Created,
    Updated,
    Unchanged,
}

/// Aggregate report for a single `install` / `update` pass over the
/// Claude registration files. Kept as a struct (rather than a single
/// `WriteAction`) so the CLI can attribute each writeable file
/// separately when more land in the future.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WireReport {
    pub settings: WriteAction,
}

/// Merge HEAL's hook entries into the project's `settings.json`. The
/// merge is additive — existing user hooks are preserved.
pub fn wire(project: &Path) -> Result<WireReport> {
    let settings = register(project)?;
    Ok(WireReport { settings })
}

/// Idempotent settings.json reconciliation. Modern HEAL doesn't add
/// any Claude Code hooks, so this only sweeps legacy command entries
/// (`heal hook edit`, `heal hook stop`) left over from earlier
/// installs. A nonexistent settings file stays nonexistent.
pub fn register(project: &Path) -> Result<WriteAction> {
    let path = project.join(SETTINGS_FILE);
    let Ok(prior) = std::fs::read_to_string(&path) else {
        return Ok(WriteAction::Unchanged);
    };
    let mut value: Value =
        serde_json::from_str(&prior).with_context(|| format!("parsing {}", path.display()))?;
    remove_heal_hooks(&mut value);
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(&value).expect("settings serialization is infallible")
    );
    if body == prior {
        return Ok(WriteAction::Unchanged);
    }
    write_if_changed(&path, &body)
}

/// Outcome of [`unregister`]. `legacy_swept` is true when at least one
/// pre-skills artifact (plugin tree, marketplace.json, or settings-key)
/// was actually removed during this call — distinct from "would be
/// removed" so callers can surface honest UX text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnregisterReport {
    pub legacy_swept: bool,
}

/// Remove HEAL's hook entries from `settings.json`, plus any legacy
/// marketplace / `enabledPlugins` keys and the old plugin/marketplace
/// files on disk. User entries (anything whose `command` doesn't match
/// a HEAL hook) survive untouched. The file itself is removed when
/// nothing else remains.
pub fn unregister(project: &Path) -> Result<UnregisterReport> {
    let mut legacy_swept = remove_legacy_artifacts(project)?;
    let settings_path = project.join(SETTINGS_FILE);
    let Ok(prior) = std::fs::read_to_string(&settings_path) else {
        return Ok(UnregisterReport { legacy_swept });
    };
    let mut value: Value = serde_json::from_str(&prior)
        .with_context(|| format!("parsing {}", settings_path.display()))?;
    remove_heal_hooks(&mut value);
    legacy_swept |= remove_legacy_settings_keys(&mut value);
    if value.as_object().is_some_and(serde_json::Map::is_empty) {
        std::fs::remove_file(&settings_path)
            .with_context(|| format!("removing {}", settings_path.display()))?;
        return Ok(UnregisterReport { legacy_swept });
    }
    let cleaned = format!(
        "{}\n",
        serde_json::to_string_pretty(&value).expect("settings serialization is infallible")
    );
    if cleaned != prior {
        crate::core::fs::atomic_write(&settings_path, cleaned.as_bytes())
            .with_context(|| format!("writing {}", settings_path.display()))?;
    }
    Ok(UnregisterReport { legacy_swept })
}

/// Sweep on-disk artifacts left over from the old plugin/marketplace
/// install layout. Returns `true` when at least one path was removed.
/// Idempotent: missing paths are silently treated as no-ops via
/// `ErrorKind::NotFound` suppression rather than a pre-`exists()` check
/// (which would race in the unlikely case of a concurrent writer).
fn remove_legacy_artifacts(project: &Path) -> Result<bool> {
    let mut swept = false;
    swept |= remove_dir_all_if_present(&project.join(LEGACY_PLUGIN_DEST_REL))?;
    swept |= remove_file_if_present(&project.join(LEGACY_MARKETPLACE_FILE))?;
    let market_dir = project.join(LEGACY_MARKETPLACE_DIR);
    if market_dir.is_dir() {
        // Best-effort: leave the dir if a sibling marketplace is in there.
        let _ = crate::core::fs::remove_dir_if_empty(&market_dir);
    }
    Ok(swept)
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
/// value. Empty parent objects are dropped after removal. Returns
/// `true` when at least one key was present (i.e. legacy state
/// existed and was just removed).
fn remove_legacy_settings_keys(value: &mut Value) -> bool {
    let Some(obj) = value.as_object_mut() else {
        return false;
    };
    let mut found = false;
    if let Some(market) = obj
        .get_mut("extraKnownMarketplaces")
        .and_then(Value::as_object_mut)
    {
        if market.remove(LEGACY_MARKETPLACE_NAME).is_some() {
            found = true;
        }
        if market.is_empty() {
            obj.remove("extraKnownMarketplaces");
        }
    }
    if let Some(enabled) = obj.get_mut("enabledPlugins").and_then(Value::as_object_mut) {
        if enabled.remove(LEGACY_ENABLED_PLUGIN_KEY).is_some() {
            found = true;
        }
        if enabled.is_empty() {
            obj.remove("enabledPlugins");
        }
    }
    found
}

/// Idempotent atomic write: skip when on-disk bytes already match
/// `body`, otherwise route through `core::fs::atomic_write` so a SIGINT
/// mid-write can't leave `settings.json` half-written.
fn write_if_changed(path: &Path, body: &str) -> Result<WriteAction> {
    let prior = std::fs::read_to_string(path).ok();
    if prior.as_deref() == Some(body) {
        return Ok(WriteAction::Unchanged);
    }
    crate::core::fs::atomic_write(path, body.as_bytes())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(if prior.is_some() {
        WriteAction::Updated
    } else {
        WriteAction::Created
    })
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
    fn register_is_noop_when_settings_absent() {
        let dir = TempDir::new().unwrap();
        let action = register(dir.path()).unwrap();
        assert_eq!(action, WriteAction::Unchanged);
        assert!(!dir.path().join(SETTINGS_FILE).exists());
    }

    #[test]
    fn register_sweeps_legacy_heal_hook_commands() {
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
        register(dir.path()).unwrap();
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        // User entries survive.
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
    fn register_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let first = register(dir.path()).unwrap();
        let second = register(dir.path()).unwrap();
        assert_eq!(first, WriteAction::Unchanged);
        assert_eq!(second, WriteAction::Unchanged);
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
        std::fs::write(&market, "{}").unwrap();

        unregister(dir.path()).unwrap();
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
}
