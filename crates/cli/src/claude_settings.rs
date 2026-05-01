//! Wire the bundled Claude plugin into Claude Code's discovery path.
//!
//! `plugin_assets::extract` drops the plugin tree at
//! `.claude/plugins/heal/`, but Claude Code only loads plugins reachable
//! from a registered marketplace. We synthesize a single-entry
//! marketplace at `.claude-plugin/marketplace.json` whose relative
//! `source` points back at the extracted tree, then merge two keys into
//! `.claude/settings.json` (`extraKnownMarketplaces` +
//! `enabledPlugins`) so the marketplace auto-loads on the next session
//! and the plugin is enabled by default.
//!
//! Settings merging is surgical: unrelated user keys (theme, hooks,
//! permissions) are preserved byte-for-byte through a `serde_json::Value`
//! round-trip. `unregister` removes only our entries and deletes the
//! file when it would otherwise be left with an empty object.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::plugin_assets::PLUGIN_DEST_REL;

const MARKETPLACE_NAME: &str = "heal-local";
const PLUGIN_NAME: &str = "heal";
const MARKETPLACE_FILE: &str = ".claude-plugin/marketplace.json";
const SETTINGS_FILE: &str = ".claude/settings.json";

/// Outcome reported by [`register`] / [`write_marketplace`]. Drives the
/// CLI status line and lets callers report no-op vs. mutation distinctly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteAction {
    Created,
    Updated,
    Unchanged,
}

/// Aggregate report for a single `install` / `update` pass over the
/// Claude registration files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireReport {
    pub marketplace: WriteAction,
    pub settings: WriteAction,
}

/// Run both registration steps. Convenience for the common case where
/// extraction succeeded and we want Claude Code to see the plugin.
pub fn wire(project: &Path, plugin_version: &str) -> Result<WireReport> {
    let marketplace = write_marketplace(project, plugin_version)?;
    let settings = register(project)?;
    Ok(WireReport {
        marketplace,
        settings,
    })
}

/// Write the project-local marketplace catalog. The marketplace name is
/// fixed (`heal-local`) so re-runs converge; the plugin entry's
/// `version` reflects the bundled `plugin.json` so updates flow through.
pub fn write_marketplace(project: &Path, plugin_version: &str) -> Result<WriteAction> {
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(&marketplace_value(plugin_version))
            .expect("marketplace serialization is infallible")
    );
    write_if_changed(&project.join(MARKETPLACE_FILE), &body)
}

/// Merge our marketplace + enabled-plugin entries into the project's
/// `settings.json`, preserving any unrelated user keys.
pub fn register(project: &Path) -> Result<WriteAction> {
    let path = project.join(SETTINGS_FILE);
    let prior = std::fs::read_to_string(&path).ok();
    let mut value: Value = match prior.as_deref() {
        Some(s) => {
            serde_json::from_str(s).with_context(|| format!("parsing {}", path.display()))?
        }
        None => json!({}),
    };
    upsert_settings(&mut value);
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(&value).expect("settings serialization is infallible")
    );
    write_if_changed(&path, &body)
}

/// Idempotent file write: skip when on-disk bytes already match `body`,
/// otherwise create the parent directory and write. Distinguishes
/// `Created` (no prior file) from `Updated` (prior file with different
/// bytes) so callers can render an honest status line.
fn write_if_changed(path: &Path, body: &str) -> Result<WriteAction> {
    let prior = std::fs::read_to_string(path).ok();
    if prior.as_deref() == Some(body) {
        return Ok(WriteAction::Unchanged);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    std::fs::write(path, body).with_context(|| format!("writing {}", path.display()))?;
    Ok(if prior.is_some() {
        WriteAction::Updated
    } else {
        WriteAction::Created
    })
}

/// Reverse of [`wire`]. Removes the marketplace file and our entries
/// from `settings.json`, then deletes the settings file (and the empty
/// `.claude-plugin/` parent) if no other keys remain.
pub fn unregister(project: &Path) -> Result<()> {
    let market_path = project.join(MARKETPLACE_FILE);
    if market_path.exists() {
        std::fs::remove_file(&market_path)
            .with_context(|| format!("removing {}", market_path.display()))?;
    }
    let market_parent = project.join(".claude-plugin");
    if market_parent.is_dir() && dir_is_empty(&market_parent) {
        let _ = std::fs::remove_dir(&market_parent);
    }

    let settings_path = project.join(SETTINGS_FILE);
    let Ok(prior) = std::fs::read_to_string(&settings_path) else {
        return Ok(());
    };
    let mut value: Value = serde_json::from_str(&prior)
        .with_context(|| format!("parsing {}", settings_path.display()))?;
    remove_from_settings(&mut value);
    if value.as_object().is_some_and(serde_json::Map::is_empty) {
        std::fs::remove_file(&settings_path)
            .with_context(|| format!("removing {}", settings_path.display()))?;
        return Ok(());
    }
    let cleaned = format!(
        "{}\n",
        serde_json::to_string_pretty(&value).expect("settings serialization is infallible")
    );
    if cleaned != prior {
        std::fs::write(&settings_path, &cleaned)
            .with_context(|| format!("writing {}", settings_path.display()))?;
    }
    Ok(())
}

fn dir_is_empty(p: &Path) -> bool {
    std::fs::read_dir(p).is_ok_and(|mut it| it.next().is_none())
}

fn marketplace_value(plugin_version: &str) -> Value {
    json!({
        "$schema": "https://anthropic.com/claude-code/marketplace.schema.json",
        "name": MARKETPLACE_NAME,
        "description": "HEAL — local marketplace generated by `heal skills install`.",
        "owner": {"name": "heal-cli"},
        "plugins": [
            {
                "name": PLUGIN_NAME,
                "description": "HEAL — code health monitoring & maintenance via Claude Code hooks and skills.",
                "version": plugin_version,
                "source": format!("./{PLUGIN_DEST_REL}"),
            }
        ]
    })
}

fn upsert_settings(value: &mut Value) {
    let obj = ensure_object(value);

    let market = obj
        .entry("extraKnownMarketplaces")
        .or_insert_with(|| json!({}));
    if let Some(map) = market.as_object_mut() {
        map.insert(MARKETPLACE_NAME.to_string(), settings_marketplace_entry());
    }

    let enabled = obj.entry("enabledPlugins").or_insert_with(|| json!({}));
    if let Some(map) = enabled.as_object_mut() {
        map.insert(enabled_key(), Value::Bool(true));
    }
}

fn settings_marketplace_entry() -> Value {
    json!({
        "source": {
            "source": "file",
            "path": format!("./{MARKETPLACE_FILE}"),
        }
    })
}

fn remove_from_settings(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    if let Some(market) = obj
        .get_mut("extraKnownMarketplaces")
        .and_then(Value::as_object_mut)
    {
        market.remove(MARKETPLACE_NAME);
        if market.is_empty() {
            obj.remove("extraKnownMarketplaces");
        }
    }
    if let Some(enabled) = obj.get_mut("enabledPlugins").and_then(Value::as_object_mut) {
        enabled.remove(&enabled_key());
        if enabled.is_empty() {
            obj.remove("enabledPlugins");
        }
    }
}

fn enabled_key() -> String {
    format!("{PLUGIN_NAME}@{MARKETPLACE_NAME}")
}

fn ensure_object(value: &mut Value) -> &mut serde_json::Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("just inserted an object")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn marketplace_payload_pins_relative_source() {
        let v = marketplace_value("0.2.0");
        assert_eq!(v["name"], MARKETPLACE_NAME);
        let plugin = &v["plugins"][0];
        assert_eq!(plugin["name"], PLUGIN_NAME);
        assert_eq!(plugin["source"], format!("./{PLUGIN_DEST_REL}"));
        assert_eq!(plugin["version"], "0.2.0");
    }

    #[test]
    fn write_marketplace_creates_file_then_no_ops() {
        let dir = TempDir::new().unwrap();
        let first = write_marketplace(dir.path(), "0.2.0").unwrap();
        assert_eq!(first, WriteAction::Created);
        let second = write_marketplace(dir.path(), "0.2.0").unwrap();
        assert_eq!(second, WriteAction::Unchanged);
        let body = std::fs::read_to_string(dir.path().join(MARKETPLACE_FILE)).unwrap();
        assert!(body.contains("\"heal-local\""));
    }

    #[test]
    fn write_marketplace_bumps_version() {
        let dir = TempDir::new().unwrap();
        write_marketplace(dir.path(), "0.1.0").unwrap();
        let action = write_marketplace(dir.path(), "0.2.0").unwrap();
        assert_eq!(action, WriteAction::Updated);
        let body = std::fs::read_to_string(dir.path().join(MARKETPLACE_FILE)).unwrap();
        assert!(body.contains("0.2.0"));
    }

    #[test]
    fn register_creates_settings_when_absent() {
        let dir = TempDir::new().unwrap();
        let action = register(dir.path()).unwrap();
        assert_eq!(action, WriteAction::Created);
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap())
                .unwrap();
        assert_eq!(v["enabledPlugins"]["heal@heal-local"], true);
        assert_eq!(
            v["extraKnownMarketplaces"]["heal-local"]["source"]["source"],
            "file"
        );
        assert_eq!(
            v["extraKnownMarketplaces"]["heal-local"]["source"]["path"],
            format!("./{MARKETPLACE_FILE}")
        );
    }

    #[test]
    fn register_preserves_existing_user_keys() {
        let dir = TempDir::new().unwrap();
        let settings_path = dir.path().join(SETTINGS_FILE);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{"theme":"dark","permissions":{"allow":["Bash(ls *)"]},"enabledPlugins":{"other@x":true}}"#,
        )
        .unwrap();
        register(dir.path()).unwrap();
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["permissions"]["allow"][0], "Bash(ls *)");
        assert_eq!(v["enabledPlugins"]["other@x"], true);
        assert_eq!(v["enabledPlugins"]["heal@heal-local"], true);
    }

    #[test]
    fn register_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let first = register(dir.path()).unwrap();
        let second = register(dir.path()).unwrap();
        assert_eq!(first, WriteAction::Created);
        assert_eq!(second, WriteAction::Unchanged);
    }

    #[test]
    fn unregister_strips_only_our_entries() {
        let dir = TempDir::new().unwrap();
        let settings_path = dir.path().join(SETTINGS_FILE);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{"theme":"dark","enabledPlugins":{"heal@heal-local":true,"other@x":true}}"#,
        )
        .unwrap();
        write_marketplace(dir.path(), "0.2.0").unwrap();
        unregister(dir.path()).unwrap();
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["enabledPlugins"]["other@x"], true);
        assert!(v["enabledPlugins"].get("heal@heal-local").is_none());
        assert!(v.get("extraKnownMarketplaces").is_none());
        assert!(!dir.path().join(MARKETPLACE_FILE).exists());
        assert!(!dir.path().join(".claude-plugin").exists());
    }

    #[test]
    fn unregister_deletes_settings_when_empty() {
        let dir = TempDir::new().unwrap();
        register(dir.path()).unwrap();
        unregister(dir.path()).unwrap();
        assert!(!dir.path().join(SETTINGS_FILE).exists());
    }

    #[test]
    fn unregister_when_nothing_was_wired_is_noop() {
        let dir = TempDir::new().unwrap();
        unregister(dir.path()).unwrap();
        assert!(!dir.path().join(SETTINGS_FILE).exists());
        assert!(!dir.path().join(MARKETPLACE_FILE).exists());
    }
}
