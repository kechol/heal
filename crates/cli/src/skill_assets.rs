//! Build-time embedding of the bundled skill set
//! (`crates/cli/plugins/heal/skills/`) and the install/update bookkeeping
//! that surrounds it.
//!
//! The embedded tree's children are individual skill directories
//! (`heal-cli/`, `heal-config/`, `heal-code-review/`, `heal-code-patch/`)
//! that get extracted directly into `<project>/.claude/skills/<name>/`.
//! No marketplace, no plugin wrapper — Claude Code natively discovers
//! project-scope skills under `.claude/skills/`.
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
//! The manifest lives at `.heal/skills-install.json` (heal-owned state,
//! decoupled from `.claude/`). Drift detection compares an on-disk
//! fingerprint against the manifest's recorded fingerprint from the
//! previous install.
//!
//! The fingerprint algorithm is a hand-rolled FNV-1a 64-bit hash formatted
//! as 16 hex digits. It is *not* cryptographic — its only job is to
//! distinguish "byte-for-byte identical to last install" from "edited."
//! FNV is preferred over `std::hash::DefaultHasher` because the std hasher
//! is explicitly unstable across Rust toolchain versions, which would
//! invalidate every recorded fingerprint after a `rustc` upgrade.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use include_dir::{include_dir, Dir, DirEntry, File};
use serde::{Deserialize, Serialize};

/// Embedded bundle. Each top-level child is a skill directory whose
/// contents land 1:1 under `<project>/.claude/skills/<skill-name>/`.
pub static SKILLS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/plugins/heal/skills");

/// Project-relative location of the extracted skills tree. Single source
/// of truth for the install destination.
pub const SKILLS_DEST_REL: &str = ".claude/skills";

/// Resolve the skills destination directory inside `project`.
#[must_use]
pub fn skills_dest(project: &Path) -> PathBuf {
    project.join(SKILLS_DEST_REL)
}

/// Source-of-install marker recorded in the manifest.
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

#[derive(Debug, Default, Clone, Copy, Serialize)]
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
    /// Map of `<skill-name>/<rel-path>` → fingerprint hex. The relative
    /// path uses `/` separators regardless of host OS.
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

    /// Read the manifest from its on-disk path. `None` when the file is
    /// missing or unparseable — callers treat both as "no prior state."
    pub fn load(path: &Path) -> Option<Self> {
        let body = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&body).ok()
    }

    fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir {}", parent.display()))?;
        }
        let body = serde_json::to_string_pretty(self)
            .expect("InstallManifest serialization is infallible");
        std::fs::write(path, body).with_context(|| format!("writing {}", path.display()))
    }
}

/// Bundled HEAL version. Sourced from the crate's `Cargo.toml` at build
/// time so install metadata always matches the binary that wrote it.
#[must_use]
pub fn bundled_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Walk the embedded tree, write each entry to `dest` (the skills
/// parent directory), persist the manifest at `manifest_path`, and
/// return both the per-file outcome and the manifest that was just
/// persisted.
pub fn extract(
    dest: &Path,
    manifest_path: &Path,
    mode: ExtractMode,
) -> Result<(ExtractStats, InstallManifest)> {
    std::fs::create_dir_all(dest)
        .with_context(|| format!("creating skills dest dir {}", dest.display()))?;

    let prior = InstallManifest::load(manifest_path);
    let version = bundled_version();
    let mut manifest = InstallManifest::new(version.clone(), Utc::now());
    let install_meta = SkillInstallMeta {
        version,
        source: INSTALL_SOURCE_BUNDLED.to_string(),
    };
    let mut stats = ExtractStats::default();

    walk(
        &SKILLS_DIR,
        dest,
        mode,
        prior.as_ref(),
        &install_meta,
        &mut stats,
        &mut manifest,
    )?;
    manifest.save(manifest_path)?;
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
/// keep flagging it as "updated". The install timestamp lives in
/// [`InstallManifest::installed_at`] instead.
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

    fn manifest_under(dir: &Path) -> PathBuf {
        dir.join("skills-install.json")
    }

    #[test]
    fn bundled_version_returns_crate_version() {
        assert_eq!(bundled_version(), env!("CARGO_PKG_VERSION"));
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
        let dest = dir.path().join("skills");
        let manifest = manifest_under(dir.path());
        let (stats1, _) = extract(&dest, &manifest, ExtractMode::InstallSafe).unwrap();
        assert!(stats1.added.iter().any(|p| p == "heal-cli/SKILL.md"));
        let target = dest.join("heal-code-patch/SKILL.md");
        std::fs::write(&target, "---\nuser edit\n---\n").unwrap();
        let (stats2, _) = extract(&dest, &manifest, ExtractMode::InstallSafe).unwrap();
        assert!(stats2
            .skipped
            .iter()
            .any(|p| p == "heal-code-patch/SKILL.md"));
        let body = std::fs::read_to_string(&target).unwrap();
        assert!(body.contains("user edit"));
    }

    #[test]
    fn extract_update_skips_user_modified_without_force() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("skills");
        let manifest = manifest_under(dir.path());
        extract(&dest, &manifest, ExtractMode::InstallSafe).unwrap();

        let target = dest.join("heal-code-patch/SKILL.md");
        std::fs::write(&target, "---\nuser edit\n---\n").unwrap();

        let (stats, _) = extract(&dest, &manifest, ExtractMode::Update { force: false }).unwrap();
        assert!(stats
            .user_modified
            .iter()
            .any(|p| p == "heal-code-patch/SKILL.md"));
        let body = std::fs::read_to_string(&target).unwrap();
        assert!(body.contains("user edit"));
    }

    #[test]
    fn extract_update_force_overwrites_user_edits() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("skills");
        let manifest = manifest_under(dir.path());
        extract(&dest, &manifest, ExtractMode::InstallSafe).unwrap();

        let target = dest.join("heal-code-patch/SKILL.md");
        std::fs::write(&target, "---\nuser edit\n---\n").unwrap();

        let (stats, _) = extract(&dest, &manifest, ExtractMode::Update { force: true }).unwrap();
        assert!(stats
            .updated
            .iter()
            .any(|p| p == "heal-code-patch/SKILL.md"));
        let body = std::fs::read_to_string(&target).unwrap();
        assert!(!body.contains("user edit"));
    }

    #[test]
    fn install_manifest_records_version_and_assets() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("skills");
        let manifest_path = manifest_under(dir.path());
        let (_, manifest) = extract(&dest, &manifest_path, ExtractMode::InstallSafe).unwrap();
        assert_eq!(manifest.heal_version, bundled_version());
        assert_eq!(manifest.source, "bundled");
        assert!(manifest.assets.contains_key("heal-cli/SKILL.md"));
        let loaded = InstallManifest::load(&manifest_path).unwrap();
        assert_eq!(loaded, manifest);
    }

    #[test]
    fn skill_md_install_carries_frontmatter_metadata() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("skills");
        let manifest = manifest_under(dir.path());
        extract(&dest, &manifest, ExtractMode::InstallSafe).unwrap();
        let body = std::fs::read_to_string(dest.join("heal-code-review/SKILL.md")).unwrap();
        assert!(body.contains("metadata:"));
        assert!(body.contains(&format!("heal-version: {}", bundled_version())));
    }
}
