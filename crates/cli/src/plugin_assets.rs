//! Build-time embedding of the Claude plugin tree (`crates/cli/plugins/heal/`)
//! and the install/update bookkeeping that surrounds it.
//!
//! The plugin tree lives inside the `heal-cli` crate directory so a published
//! crates.io tarball includes it — `include_dir!` is a compile-time read,
//! and Cargo only packages files inside the crate directory.
//!
//! ## Why a manifest?
//!
//! `heal skills update` needs three things the embedded tree alone can't
//! tell it:
//!   1. Which version was last installed (so `status` can compare against
//!      the bundled version).
//!   2. Which files the user has edited locally since the last install
//!      (so a routine `update` doesn't blow away hand-tuned skills).
//!   3. A timestamp + source provenance per agentskills.io conventions.
//!
//! HEAL records (1) and (3) in `.heal-install.json` at the plugin root and
//! mirrors the same metadata into each SKILL.md's YAML frontmatter so the
//! file remains self-describing if it leaves HEAL's directory tree.
//! Drift detection (2) compares an on-disk fingerprint with the manifest's
//! recorded fingerprint from the previous install.
//!
//! The fingerprint algorithm is a hand-rolled FNV-1a 64-bit hash formatted
//! as 16 hex digits. It is *not* cryptographic — its only job is to
//! distinguish "byte-for-byte identical to last install" from "edited."
//! FNV is preferred over `std::hash::DefaultHasher` because the std hasher
//! is explicitly unstable across Rust toolchain versions, which would
//! invalidate every recorded fingerprint after a `rustc` upgrade.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use include_dir::{include_dir, Dir, DirEntry, File};
use serde::{Deserialize, Serialize};

pub static PLUGIN_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/plugins/heal");

/// Filename of the install metadata, stored at the plugin root.
pub const INSTALL_MANIFEST: &str = ".heal-install.json";

/// Source-of-install marker recorded in the manifest. Static for v0.1
/// (Marketplace adds another value in v0.3+).
pub const INSTALL_SOURCE_BUNDLED: &str = "bundled";

/// Caller intent for [`extract`]. Drift handling differs between the three:
///
/// - [`ExtractMode::InstallSafe`]: leave existing files alone (matches the
///   default `heal skills install` ergonomics — initial install or noop).
/// - [`ExtractMode::InstallForce`]: overwrite every file, drift or not.
/// - [`ExtractMode::Update { force }`]: overwrite *unchanged* files; skip
///   files whose on-disk fingerprint diverges from the prior manifest
///   unless `force` is true (matches `heal skills update [--force]`).
#[derive(Debug, Clone, Copy)]
pub enum ExtractMode {
    InstallSafe,
    InstallForce,
    Update { force: bool },
}

/// Per-asset outcome of an extract pass. `summary()` collapses the lists
/// into counts for the CLI status line; the lists themselves drive the
/// "modified locally:" detail rows.
#[derive(Debug, Default, Clone)]
pub struct ExtractStats {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
    pub skipped: Vec<String>,
    pub user_modified: Vec<String>,
}

impl ExtractStats {
    #[must_use]
    pub fn summary(&self) -> ExtractSummary {
        ExtractSummary {
            added: self.added.len(),
            updated: self.updated.len(),
            unchanged: self.unchanged.len(),
            skipped: self.skipped.len(),
            user_modified: self.user_modified.len(),
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ExtractSummary {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub user_modified: usize,
}

/// On-disk record of "what was last installed". Read by `heal skills
/// status` and by `update` for drift detection. Forward-compatible —
/// readers ignore unknown fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallManifest {
    pub heal_version: String,
    pub installed_at: DateTime<Utc>,
    pub source: String,
    /// Map of `<relative-path>` → fingerprint hex. The relative path uses
    /// `/` separators regardless of host OS.
    pub assets: BTreeMap<String, String>,
}

impl InstallManifest {
    fn new(version: String, now: DateTime<Utc>) -> Self {
        Self {
            heal_version: version,
            installed_at: now,
            source: INSTALL_SOURCE_BUNDLED.to_string(),
            assets: BTreeMap::new(),
        }
    }

    pub fn load(plugin_root: &Path) -> Option<Self> {
        let path = plugin_root.join(INSTALL_MANIFEST);
        let body = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&body).ok()
    }

    fn save(&self, plugin_root: &Path) -> Result<()> {
        let path = plugin_root.join(INSTALL_MANIFEST);
        let body = serde_json::to_string_pretty(self)
            .expect("InstallManifest serialization is infallible");
        std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))
    }
}

/// Read the embedded `plugin.json` and pull `version` out. `None` when the
/// embedded tree is malformed (shouldn't happen in practice — the file is
/// shipped with the binary).
#[must_use]
pub fn bundled_version() -> Option<String> {
    let file = PLUGIN_DIR.get_file("plugin.json")?;
    let body = std::str::from_utf8(file.contents()).ok()?;
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("version")?.as_str().map(str::to_string)
}

/// Walk the embedded tree, write each entry to `dest`, and return both
/// the per-file outcome and the manifest that was just persisted. The
/// manifest is also written to `dest/.heal-install.json` as a side effect.
pub fn extract(dest: &Path, mode: ExtractMode) -> Result<(ExtractStats, InstallManifest)> {
    std::fs::create_dir_all(dest)
        .with_context(|| format!("creating plugin dest dir {}", dest.display()))?;

    let prior = InstallManifest::load(dest);
    let version = bundled_version().unwrap_or_else(|| "unknown".to_string());
    let mut manifest = InstallManifest::new(version.clone(), Utc::now());
    let install_meta = SkillInstallMeta {
        version,
        source: INSTALL_SOURCE_BUNDLED.to_string(),
    };
    let mut stats = ExtractStats::default();

    walk(
        &PLUGIN_DIR,
        dest,
        mode,
        prior.as_ref(),
        &install_meta,
        &mut stats,
        &mut manifest,
    )?;
    manifest.save(dest)?;
    Ok((stats, manifest))
}

/// Recursive worker for [`extract`]. Walks `dir` (an embedded directory)
/// and writes each file under `dest`, applying the per-mode policy.
fn walk(
    dir: &Dir<'_>,
    dest: &Path,
    mode: ExtractMode,
    prior: Option<&InstallManifest>,
    meta: &SkillInstallMeta,
    stats: &mut ExtractStats,
    manifest: &mut InstallManifest,
) -> Result<()> {
    for entry in dir.entries() {
        let rel_path = entry.path();
        let target = dest.join(rel_path);
        match entry {
            DirEntry::Dir(child) => {
                std::fs::create_dir_all(&target)
                    .with_context(|| format!("mkdir {}", target.display()))?;
                walk(child, dest, mode, prior, meta, stats, manifest)?;
            }
            DirEntry::File(file) => {
                handle_file(file, &target, rel_path, mode, prior, meta, stats, manifest)?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_file(
    file: &File<'_>,
    target: &Path,
    rel_path: &Path,
    mode: ExtractMode,
    prior: Option<&InstallManifest>,
    meta: &SkillInstallMeta,
    stats: &mut ExtractStats,
    manifest: &mut InstallManifest,
) -> Result<()> {
    let rel_key = relative_key(rel_path);
    let body = canonical_bytes(file, rel_path, meta);
    let new_fp = fingerprint(&body);
    manifest.assets.insert(rel_key.clone(), new_fp.clone());

    if !target.exists() {
        write_asset(target, &body)?;
        stats.added.push(rel_key);
        return Ok(());
    }

    match mode {
        ExtractMode::InstallSafe => {
            stats.skipped.push(rel_key);
            return Ok(());
        }
        ExtractMode::InstallForce => {
            let pre_fp = fingerprint(&std::fs::read(target).unwrap_or_default());
            write_asset(target, &body)?;
            classify(stats, &pre_fp, &new_fp, rel_key);
            return Ok(());
        }
        ExtractMode::Update { force } => {
            let pre_fp = fingerprint(&std::fs::read(target).unwrap_or_default());
            let prior_fp = prior.and_then(|m| m.assets.get(&rel_key)).cloned();
            let drifted = prior_fp.map_or(pre_fp != new_fp, |p| pre_fp != p);
            if drifted && !force {
                stats.user_modified.push(rel_key);
                return Ok(());
            }
            write_asset(target, &body)?;
            classify(stats, &pre_fp, &new_fp, rel_key);
        }
    }
    Ok(())
}

/// Fold a just-written asset into either `unchanged` or `updated` by
/// comparing the file's pre-write fingerprint against the bundled bytes
/// we just emitted — i.e. did the on-disk content actually change?
fn classify(stats: &mut ExtractStats, pre_fp: &str, new_fp: &str, rel_key: String) {
    if pre_fp == new_fp {
        stats.unchanged.push(rel_key);
    } else {
        stats.updated.push(rel_key);
    }
}

fn write_asset(target: &Path, body: &[u8]) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    std::fs::write(target, body).with_context(|| format!("writing {}", target.display()))?;
    #[cfg(unix)]
    if target.extension().is_some_and(|ext| ext == "sh") {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(target)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(target, perms)?;
    }
    Ok(())
}

/// Bytes we *actually* write to disk. SKILL.md gets a metadata block
/// merged into its YAML frontmatter; everything else is verbatim.
fn canonical_bytes(file: &File<'_>, rel_path: &Path, meta: &SkillInstallMeta) -> Vec<u8> {
    let raw = file.contents();
    if rel_path.file_name().is_some_and(|n| n == "SKILL.md") {
        if let Ok(text) = std::str::from_utf8(raw) {
            return inject_skill_metadata(text, meta).into_bytes();
        }
    }
    raw.to_vec()
}

/// Per-skill frontmatter metadata. Intentionally **content-derived only**
/// — no timestamps. A wall-clock value here would make every `extract`
/// pass produce different bytes for an otherwise-unchanged SKILL.md and
/// keep flagging it as "updated". The plugin-wide install timestamp
/// lives in [`InstallManifest::installed_at`] instead.
#[derive(Debug, Clone)]
struct SkillInstallMeta {
    version: String,
    source: String,
}

/// Inject a `metadata:` block at the end of the leading YAML frontmatter.
/// Idempotent: if the input already carries `metadata:` lines, they are
/// dropped first so re-installs don't accumulate stale entries.
///
/// We deliberately avoid a YAML parser dep — the source frontmatter is
/// simple flat YAML under HEAL's control, and the merge happens line-wise.
fn inject_skill_metadata(body: &str, meta: &SkillInstallMeta) -> String {
    if !body.starts_with("---\n") {
        return body.to_string();
    }
    let after_open = &body[4..];
    let Some(close_offset) = after_open.find("\n---\n") else {
        return body.to_string();
    };
    let frontmatter = &after_open[..close_offset];
    let rest = &after_open[close_offset + 5..]; // skip "\n---\n"

    let mut kept_lines: Vec<&str> = Vec::new();
    let mut in_metadata = false;
    for line in frontmatter.lines() {
        if in_metadata {
            // Members of the previous metadata block are indented; the
            // first un-indented (non-empty) line resumes regular keys.
            if line.starts_with(' ') || line.is_empty() {
                continue;
            }
            in_metadata = false;
        }
        if line.trim_start().starts_with("metadata:") {
            in_metadata = true;
            continue;
        }
        kept_lines.push(line);
    }

    let mut out = String::with_capacity(body.len() + 200);
    out.push_str("---\n");
    for line in &kept_lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("metadata:\n");
    let _ = writeln!(out, "  heal-version: {}", meta.version);
    let _ = writeln!(out, "  heal-source: {}", meta.source);
    out.push_str("---\n");
    out.push_str(rest);
    out
}

/// Drift-detection fingerprint shared by `extract` (manifest writer) and
/// `skills::status` (drift reader). Backed by the workspace-shared
/// FNV-1a 64-bit so the same bytes always map to the same hex digest
/// across processes and toolchain versions.
pub(crate) fn fingerprint(bytes: &[u8]) -> String {
    crate::core::hash::fnv1a_hex(crate::core::hash::fnv1a_64(bytes))
}

fn relative_key(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fixed_meta() -> SkillInstallMeta {
        SkillInstallMeta {
            version: "0.1.0".into(),
            source: "bundled".into(),
        }
    }

    #[test]
    fn bundled_version_reads_plugin_json() {
        assert_eq!(bundled_version().as_deref(), Some("0.1.0"));
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let a = fingerprint(b"hello");
        let b = fingerprint(b"hello");
        assert_eq!(a, b);
        assert_ne!(a, fingerprint(b"hellx"));
    }

    #[test]
    fn inject_metadata_inserts_block() {
        let body = "---\nname: x\ndescription: y\n---\n\n# x\n";
        let out = inject_skill_metadata(body, &fixed_meta());
        assert!(out.contains("metadata:"));
        assert!(out.contains("heal-version: 0.1.0"));
        assert!(out.contains("heal-source: bundled"));
        assert!(out.contains("# x"));
    }

    #[test]
    fn inject_metadata_is_deterministic() {
        // Identical inputs must produce byte-identical output across calls
        // — otherwise `update` would flag every SKILL.md as changed.
        let body = "---\nname: x\n---\n\nbody\n";
        assert_eq!(
            inject_skill_metadata(body, &fixed_meta()),
            inject_skill_metadata(body, &fixed_meta())
        );
    }

    #[test]
    fn inject_metadata_is_idempotent() {
        let body = "---\nname: x\n---\n\nbody\n";
        let once = inject_skill_metadata(body, &fixed_meta());
        let twice = inject_skill_metadata(&once, &fixed_meta());
        assert_eq!(once, twice, "second injection must not duplicate metadata");
    }

    #[test]
    fn extract_install_safe_preserves_existing_files() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path();
        // First install populates everything.
        let (stats1, _) = extract(dest, ExtractMode::InstallSafe).unwrap();
        assert!(stats1.added.iter().any(|p| p == "plugin.json"));
        // User edits an asset.
        let target = dest.join("hooks/claude-stop.sh");
        std::fs::write(&target, "#!/bin/sh\n# user edit\n").unwrap();
        // Re-install in safe mode skips it.
        let (stats2, _) = extract(dest, ExtractMode::InstallSafe).unwrap();
        assert!(stats2.skipped.iter().any(|p| p == "hooks/claude-stop.sh"));
        let body = std::fs::read_to_string(&target).unwrap();
        assert!(body.contains("user edit"));
    }

    #[test]
    fn extract_update_skips_user_modified_without_force() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path();
        extract(dest, ExtractMode::InstallSafe).unwrap();

        let target = dest.join("hooks/claude-stop.sh");
        std::fs::write(&target, "#!/bin/sh\n# user edit\n").unwrap();

        let (stats, _) = extract(dest, ExtractMode::Update { force: false }).unwrap();
        assert!(stats
            .user_modified
            .iter()
            .any(|p| p == "hooks/claude-stop.sh"));
        let body = std::fs::read_to_string(&target).unwrap();
        assert!(body.contains("user edit"));
    }

    #[test]
    fn extract_update_force_overwrites_user_edits() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path();
        extract(dest, ExtractMode::InstallSafe).unwrap();

        let target = dest.join("hooks/claude-stop.sh");
        std::fs::write(&target, "#!/bin/sh\n# user edit\n").unwrap();

        let (stats, _) = extract(dest, ExtractMode::Update { force: true }).unwrap();
        assert!(stats.updated.iter().any(|p| p == "hooks/claude-stop.sh"));
        let body = std::fs::read_to_string(&target).unwrap();
        assert!(!body.contains("user edit"));
    }

    #[test]
    fn install_manifest_records_version_and_assets() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path();
        let (_, manifest) = extract(dest, ExtractMode::InstallSafe).unwrap();
        assert_eq!(manifest.heal_version, "0.1.0");
        assert_eq!(manifest.source, "bundled");
        assert!(manifest.assets.contains_key("plugin.json"));
        // Loadable from disk too.
        let loaded = InstallManifest::load(dest).unwrap();
        assert_eq!(loaded, manifest);
    }

    #[test]
    fn skill_md_install_carries_frontmatter_metadata() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path();
        extract(dest, ExtractMode::InstallSafe).unwrap();
        let body = std::fs::read_to_string(dest.join("skills/heal-code-check/SKILL.md")).unwrap();
        assert!(body.contains("metadata:"));
        assert!(body.contains("heal-version: 0.1.0"));
    }
}
