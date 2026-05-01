//! Inject HEAL's Claude Code hooks into `<project>/.claude/settings.json`.
//!
//! Skills are extracted directly under `.claude/skills/` (handled by
//! [`crate::skill_assets`]); Claude Code discovers them natively without
//! a marketplace. This module owns the *other* half of the wiring — the
//! `Stop` and `PostToolUse` hooks that drive the HEAL event log.
//!
//! Each hook entry boils down to a single inline `command` string:
//!
//! ```jsonc
//! {
//!   "hooks": {
//!     "PostToolUse": [
//!       { "matcher": "Edit|Write|MultiEdit",
//!         "hooks": [{ "type": "command", "command": "heal hook edit" }] }
//!     ],
//!     "Stop": [
//!       { "hooks": [{ "type": "command", "command": "heal hook stop" }] }
//!     ]
//!   }
//! }
//! ```
//!
//! Merging is surgical: HEAL's own command lines are added (deduped by
//! exact match) without disturbing other entries the user wrote. On
//! `unregister`, only HEAL's command lines are removed; user blocks
//! survive untouched. Settings outside `hooks` are preserved
//! byte-for-byte through a `serde_json::Value` round-trip.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{json, Value};

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

/// Hook commands HEAL contributes. Order is meaningful only for
/// rendering / unregister-removal — the lookup is by exact `command`
/// string match.
const HEAL_HOOKS: &[HealHook] = &[
    HealHook {
        event: "PostToolUse",
        matcher: Some("Edit|Write|MultiEdit"),
        command: "heal hook edit",
    },
    HealHook {
        event: "Stop",
        matcher: None,
        command: "heal hook stop",
    },
];

#[derive(Debug, Clone, Copy)]
struct HealHook {
    event: &'static str,
    matcher: Option<&'static str>,
    command: &'static str,
}

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

/// Idempotent merge of HEAL's hook entries into `settings.json`.
pub fn register(project: &Path) -> Result<WriteAction> {
    let path = project.join(SETTINGS_FILE);
    let prior = std::fs::read_to_string(&path).ok();
    let mut value: Value = match prior.as_deref() {
        Some(s) => {
            serde_json::from_str(s).with_context(|| format!("parsing {}", path.display()))?
        }
        None => json!({}),
    };
    upsert_hooks(&mut value);
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(&value).expect("settings serialization is infallible")
    );
    write_if_changed(&path, &body)
}

/// Outcome of [`unregister`]. `legacy_swept` is true when at least one
/// pre-skills artefact (plugin tree, marketplace.json, or settings-key)
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
        std::fs::write(&settings_path, &cleaned)
            .with_context(|| format!("writing {}", settings_path.display()))?;
    }
    Ok(UnregisterReport { legacy_swept })
}

/// Sweep on-disk artefacts left over from the old plugin/marketplace
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

/// Idempotent file write: skip when on-disk bytes already match `body`,
/// otherwise create the parent directory and write.
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

/// Add each HEAL hook to its `event` array. Two-level dedupe:
///   * Block level — if a block with the same `matcher` already exists,
///     reuse it instead of appending a sibling.
///   * Inner-hook level — if an inner hook with the same `command`
///     string is already present, leave it alone.
fn upsert_hooks(value: &mut Value) {
    let obj = ensure_object(value);
    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("hooks is an object after the entry call");
    for hook in HEAL_HOOKS {
        let event_array = hooks
            .entry(hook.event.to_string())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("event slot is an array after the entry call");
        upsert_hook_in_event(event_array, hook);
    }
}

fn upsert_hook_in_event(event_array: &mut Vec<Value>, hook: &HealHook) {
    let Some(block) = event_array
        .iter_mut()
        .find(|b| matcher_of(b) == hook.matcher)
    else {
        event_array.push(new_block(hook));
        return;
    };
    ensure_inner_hooks_array(block);
    let inner = block
        .get_mut("hooks")
        .and_then(Value::as_array_mut)
        .expect("ensure_inner_hooks_array runs first");
    if inner
        .iter()
        .any(|h| h.get("command").and_then(Value::as_str) == Some(hook.command))
    {
        return;
    }
    inner.push(command_entry(hook.command));
}

/// Walk every block under every event and drop inner-hook entries whose
/// `command` matches one of HEAL's. Empty inner-hook arrays remove the
/// containing block; empty event arrays are dropped from `hooks`; an
/// empty `hooks` object is dropped from the root.
fn remove_heal_hooks(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    let Some(hooks) = obj.get_mut("hooks").and_then(Value::as_object_mut) else {
        return;
    };

    let heal_commands: Vec<&str> = HEAL_HOOKS.iter().map(|h| h.command).collect();
    for blocks in hooks.values_mut() {
        let Some(blocks) = blocks.as_array_mut() else {
            continue;
        };
        for block in blocks.iter_mut() {
            if let Some(inner) = block.get_mut("hooks").and_then(Value::as_array_mut) {
                inner.retain(|h| {
                    h.get("command")
                        .and_then(Value::as_str)
                        .is_none_or(|c| !heal_commands.contains(&c))
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

fn matcher_of(block: &Value) -> Option<&'static str> {
    // Returns &'static str when the block's matcher matches one of our
    // hooks; otherwise None — distinct from Some("") so empty matchers
    // don't accidentally collide.
    let m = block.get("matcher").and_then(Value::as_str)?;
    HEAL_HOOKS.iter().find_map(|h| {
        if h.matcher == Some(m) {
            h.matcher
        } else {
            None
        }
    })
}

fn new_block(hook: &HealHook) -> Value {
    let mut block = serde_json::Map::new();
    if let Some(m) = hook.matcher {
        block.insert("matcher".into(), Value::String(m.to_string()));
    }
    block.insert("hooks".into(), json!([command_entry(hook.command)]));
    Value::Object(block)
}

fn command_entry(command: &str) -> Value {
    json!({ "type": "command", "command": command })
}

fn ensure_inner_hooks_array(block: &mut Value) {
    let obj = block
        .as_object_mut()
        .expect("hook blocks are JSON objects by Claude's schema");
    obj.entry("hooks").or_insert_with(|| json!([]));
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
    fn register_writes_both_hook_blocks() {
        let dir = TempDir::new().unwrap();
        register(dir.path()).unwrap();
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap())
                .unwrap();
        let post = &v["hooks"]["PostToolUse"][0];
        assert_eq!(post["matcher"], "Edit|Write|MultiEdit");
        assert_eq!(post["hooks"][0]["command"], "heal hook edit");
        let stop = &v["hooks"]["Stop"][0];
        assert!(stop.get("matcher").is_none());
        assert_eq!(stop["hooks"][0]["command"], "heal hook stop");
    }

    #[test]
    fn register_preserves_user_keys() {
        let dir = TempDir::new().unwrap();
        let settings_path = dir.path().join(SETTINGS_FILE);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{"theme":"dark","permissions":{"allow":["Bash(ls *)"]}}"#,
        )
        .unwrap();
        register(dir.path()).unwrap();
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["permissions"]["allow"][0], "Bash(ls *)");
        assert_eq!(
            v["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
            "heal hook edit"
        );
    }

    #[test]
    fn register_preserves_user_hooks() {
        let dir = TempDir::new().unwrap();
        let settings_path = dir.path().join(SETTINGS_FILE);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo bye"}]}]}}"#,
        )
        .unwrap();
        register(dir.path()).unwrap();
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let stop = &v["hooks"]["Stop"];
        let commands: Vec<&str> = stop
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|b| b["hooks"].as_array().unwrap())
            .map(|h| h["command"].as_str().unwrap())
            .collect();
        assert!(commands.contains(&"echo bye"));
        assert!(commands.contains(&"heal hook stop"));
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
    fn register_dedupes_when_matcher_block_exists() {
        let dir = TempDir::new().unwrap();
        let settings_path = dir.path().join(SETTINGS_FILE);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        // Pre-existing PostToolUse block with the same matcher; HEAL must
        // append into the same block, not create a sibling.
        std::fs::write(
            &settings_path,
            r#"{"hooks":{"PostToolUse":[{"matcher":"Edit|Write|MultiEdit","hooks":[{"type":"command","command":"echo edit"}]}]}}"#,
        )
        .unwrap();
        register(dir.path()).unwrap();
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let blocks = v["hooks"]["PostToolUse"].as_array().unwrap();
        assert_eq!(
            blocks.len(),
            1,
            "HEAL must reuse the existing matcher block"
        );
        let inner = blocks[0]["hooks"].as_array().unwrap();
        let cmds: Vec<&str> = inner
            .iter()
            .map(|h| h["command"].as_str().unwrap())
            .collect();
        assert!(cmds.contains(&"echo edit"));
        assert!(cmds.contains(&"heal hook edit"));
    }

    #[test]
    fn unregister_strips_only_heal_entries() {
        let dir = TempDir::new().unwrap();
        let settings_path = dir.path().join(SETTINGS_FILE);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{"theme":"dark","hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo bye"}]}]}}"#,
        )
        .unwrap();
        register(dir.path()).unwrap();
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
        // PostToolUse had no user entry; HEAL's removal collapses the block.
        assert!(v["hooks"].get("PostToolUse").is_none());
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
