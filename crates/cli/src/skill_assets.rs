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
//! ## Drift detection without a manifest
//!
//! The bundled bytes are the source of truth. There is no
//! `skills-install.json` — every install metadata fact lives inside
//! the SKILL.md frontmatter under a `metadata:` block (heal-version,
//! heal-source). Drift is derived directly from the on-disk bytes:
//!
//!   1. `canonical(on-disk)` strips the `metadata:` block from a
//!      SKILL.md (other files are returned verbatim).
//!   2. `canonical(on-disk) == bundled raw bytes` → user has not
//!      edited; the only difference is heal's own metadata stamp.
//!   3. `canonical(on-disk) != bundled raw bytes` → user has made
//!      hand edits; `heal skills update` skips these unless `--force`.
//!
//! That makes the on-disk skill files self-describing: a teammate can
//! re-install on a different machine and the drift verdict is the same
//! function of `(on-disk bytes, bundled bytes)` no matter which machine
//! ran the previous install. No untracked manifest to coordinate.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use include_dir::{include_dir, Dir, DirEntry, File};
use serde::Serialize;

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

/// Source-of-install marker stamped into the SKILL.md `metadata:`
/// block.
pub const INSTALL_SOURCE_BUNDLED: &str = "bundled";

/// Caller intent for [`extract`]. Drift handling differs between the three:
///
/// - [`ExtractMode::InstallSafe`]: leave existing files alone (matches the
///   default `heal skills install` ergonomics — initial install or noop).
/// - [`ExtractMode::InstallForce`]: overwrite every file, drift or not.
/// - [`ExtractMode::Update { force }`]: overwrite *unmodified* files; skip
///   files whose `canonical()` content diverges from the bundled bytes
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

/// Bundled HEAL version. Sourced from the crate's `Cargo.toml` at build
/// time so install metadata always matches the binary that wrote it.
#[must_use]
pub fn bundled_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Walk the embedded tree and write each entry to `dest` (the skills
/// parent directory). Returns the per-file outcome; nothing is written
/// to a sidecar manifest — the SKILL.md frontmatter carries the install
/// metadata.
pub fn extract(dest: &Path, mode: ExtractMode) -> Result<ExtractStats> {
    std::fs::create_dir_all(dest)
        .with_context(|| format!("creating skills dest dir {}", dest.display()))?;
    let install_meta = SkillInstallMeta {
        version: bundled_version(),
        source: INSTALL_SOURCE_BUNDLED.to_string(),
    };
    let mut stats = ExtractStats::default();
    walk(&SKILLS_DIR, dest, mode, &install_meta, &mut stats)?;
    Ok(stats)
}

/// Names of every top-level skill directory in the embedded bundle —
/// the ones `heal skills install` extracts. Used by `uninstall` to
/// scope the removal: untouched user-authored skill directories under
/// `.claude/skills/` survive.
#[must_use]
pub fn bundled_skill_names() -> Vec<String> {
    SKILLS_DIR
        .entries()
        .iter()
        .filter_map(|e| match e {
            DirEntry::Dir(d) => d
                .path()
                .file_name()
                .map(|n| n.to_string_lossy().into_owned()),
            DirEntry::File(_) => None,
        })
        .collect()
}

/// Recursive worker for [`extract`]. Walks `dir` (an embedded directory)
/// and writes each file under `dest`, applying the per-mode policy.
fn walk(
    dir: &Dir<'_>,
    dest: &Path,
    mode: ExtractMode,
    meta: &SkillInstallMeta,
    stats: &mut ExtractStats,
) -> Result<()> {
    for entry in dir.entries() {
        let rel_path = entry.path();
        let target = dest.join(rel_path);
        match entry {
            DirEntry::Dir(child) => {
                std::fs::create_dir_all(&target)
                    .with_context(|| format!("mkdir {}", target.display()))?;
                walk(child, dest, mode, meta, stats)?;
            }
            DirEntry::File(file) => {
                handle_file(file, &target, rel_path, mode, meta, stats)?;
            }
        }
    }
    Ok(())
}

fn handle_file(
    file: &File<'_>,
    target: &Path,
    rel_path: &Path,
    mode: ExtractMode,
    meta: &SkillInstallMeta,
    stats: &mut ExtractStats,
) -> Result<()> {
    let rel_key = relative_key(rel_path);
    let with_metadata = canonical_bytes(file, rel_path, meta);

    if !target.exists() {
        write_asset(target, &with_metadata)?;
        stats.added.push(rel_key);
        return Ok(());
    }

    let on_disk = std::fs::read(target).unwrap_or_default();
    match mode {
        ExtractMode::InstallSafe => {
            stats.skipped.push(rel_key);
            Ok(())
        }
        ExtractMode::InstallForce => {
            classify_and_write(stats, target, &on_disk, &with_metadata, rel_key)
        }
        ExtractMode::Update { force } => {
            if user_modified(file, rel_path, &on_disk) && !force {
                stats.user_modified.push(rel_key);
                return Ok(());
            }
            classify_and_write(stats, target, &on_disk, &with_metadata, rel_key)
        }
    }
}

/// Write `with_metadata` to `target` and bucket the result. Skips the
/// disk write entirely when the bytes already match — re-running an
/// install on a clean tree shouldn't churn mtimes.
fn classify_and_write(
    stats: &mut ExtractStats,
    target: &Path,
    on_disk: &[u8],
    with_metadata: &[u8],
    rel_key: String,
) -> Result<()> {
    if on_disk == with_metadata {
        stats.unchanged.push(rel_key);
        return Ok(());
    }
    write_asset(target, with_metadata)?;
    stats.updated.push(rel_key);
    Ok(())
}

/// True when the on-disk bytes (after stripping heal's own metadata
/// block) diverge from the bundled raw bytes — the user has hand-
/// edited the file.
fn user_modified(file: &File<'_>, rel_path: &Path, on_disk: &[u8]) -> bool {
    canonical_user_bytes(rel_path, on_disk) != file.contents()
}

/// On-disk bytes with heal's own metadata stamp removed, so a
/// version-only difference doesn't read as a user edit. SKILL.md gets
/// the `metadata:` block stripped from its frontmatter; every other
/// file is returned verbatim.
fn canonical_user_bytes<'a>(rel_path: &Path, on_disk: &'a [u8]) -> std::borrow::Cow<'a, [u8]> {
    if rel_path.file_name().is_some_and(|n| n == "SKILL.md") {
        if let Ok(text) = std::str::from_utf8(on_disk) {
            return std::borrow::Cow::Owned(strip_skill_metadata(text).into_bytes());
        }
    }
    std::borrow::Cow::Borrowed(on_disk)
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
/// keep flagging it as "updated".
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

    let kept = strip_metadata_lines(frontmatter);

    let mut out = String::with_capacity(body.len() + 200);
    out.push_str("---\n");
    for line in &kept {
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

/// Strip heal's `metadata:` block from a SKILL.md so the result is
/// byte-comparable against the bundled raw source. Returns the input
/// unchanged when there's no frontmatter or no metadata block.
pub fn strip_skill_metadata(body: &str) -> String {
    if !body.starts_with("---\n") {
        return body.to_string();
    }
    let after_open = &body[4..];
    let Some(close_offset) = after_open.find("\n---\n") else {
        return body.to_string();
    };
    let frontmatter = &after_open[..close_offset];
    let rest = &after_open[close_offset + 5..];

    let kept = strip_metadata_lines(frontmatter);
    let mut out = String::with_capacity(body.len());
    out.push_str("---\n");
    for line in &kept {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("---\n");
    out.push_str(rest);
    out
}

/// Drop the `metadata:` block from a YAML frontmatter line slice.
/// Members of the block are indented; the first un-indented (non-empty)
/// line resumes regular keys.
fn strip_metadata_lines(frontmatter: &str) -> Vec<&str> {
    let mut kept: Vec<&str> = Vec::new();
    let mut in_metadata = false;
    for line in frontmatter.lines() {
        if in_metadata {
            if line.starts_with(' ') || line.is_empty() {
                continue;
            }
            in_metadata = false;
        }
        if line.trim_start().starts_with("metadata:") {
            in_metadata = true;
            continue;
        }
        kept.push(line);
    }
    kept
}

/// Read the `heal-version` value from a SKILL.md's `metadata:` block,
/// when present. Returns `None` for files without frontmatter, without
/// a `metadata:` block, or without the key.
#[must_use]
pub fn read_installed_version(skill_md_body: &str) -> Option<String> {
    if !skill_md_body.starts_with("---\n") {
        return None;
    }
    let after_open = &skill_md_body[4..];
    let close_offset = after_open.find("\n---\n")?;
    let frontmatter = &after_open[..close_offset];
    let mut in_metadata = false;
    for line in frontmatter.lines() {
        if !in_metadata {
            if line.trim_start().starts_with("metadata:") {
                in_metadata = true;
            }
            continue;
        }
        let trimmed = line.trim_start();
        if !line.starts_with(' ') && !trimmed.is_empty() {
            // Block ended without finding the key.
            return None;
        }
        if let Some(value) = trimmed.strip_prefix("heal-version:") {
            return Some(value.trim().to_string());
        }
    }
    None
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
    fn bundled_version_returns_crate_version() {
        assert_eq!(bundled_version(), env!("CARGO_PKG_VERSION"));
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
    fn strip_metadata_round_trips_through_inject() {
        let body = "---\nname: x\ndescription: y\n---\n\nbody\n";
        let injected = inject_skill_metadata(body, &fixed_meta());
        let stripped = strip_skill_metadata(&injected);
        assert_eq!(stripped, body);
    }

    #[test]
    fn read_installed_version_finds_value() {
        let body =
            "---\nname: x\nmetadata:\n  heal-version: 1.2.3\n  heal-source: bundled\n---\n\nbody\n";
        assert_eq!(read_installed_version(body), Some("1.2.3".into()));
    }

    #[test]
    fn read_installed_version_none_when_metadata_missing() {
        let body = "---\nname: x\n---\n\nbody\n";
        assert_eq!(read_installed_version(body), None);
    }

    #[test]
    fn extract_install_safe_preserves_existing_files() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("skills");
        let stats1 = extract(&dest, ExtractMode::InstallSafe).unwrap();
        assert!(stats1.added.iter().any(|p| p == "heal-cli/SKILL.md"));
        let target = dest.join("heal-code-patch/SKILL.md");
        std::fs::write(&target, "---\nuser edit\n---\n").unwrap();
        let stats2 = extract(&dest, ExtractMode::InstallSafe).unwrap();
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
        extract(&dest, ExtractMode::InstallSafe).unwrap();

        let target = dest.join("heal-code-patch/SKILL.md");
        std::fs::write(&target, "---\nuser edit\n---\n").unwrap();

        let stats = extract(&dest, ExtractMode::Update { force: false }).unwrap();
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
        extract(&dest, ExtractMode::InstallSafe).unwrap();

        let target = dest.join("heal-code-patch/SKILL.md");
        std::fs::write(&target, "---\nuser edit\n---\n").unwrap();

        let stats = extract(&dest, ExtractMode::Update { force: true }).unwrap();
        assert!(stats
            .updated
            .iter()
            .any(|p| p == "heal-code-patch/SKILL.md"));
        let body = std::fs::read_to_string(&target).unwrap();
        assert!(!body.contains("user edit"));
    }

    #[test]
    fn extract_update_unchanged_when_only_metadata_was_stripped() {
        // If the user (or another tool) wiped the metadata block but left
        // the rest intact, `update` should refresh the metadata without
        // flagging the file as user-modified.
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("skills");
        extract(&dest, ExtractMode::InstallSafe).unwrap();

        let target = dest.join("heal-code-patch/SKILL.md");
        let after_install = std::fs::read_to_string(&target).unwrap();
        let stripped = strip_skill_metadata(&after_install);
        std::fs::write(&target, &stripped).unwrap();

        let stats = extract(&dest, ExtractMode::Update { force: false }).unwrap();
        assert!(
            stats.user_modified.is_empty(),
            "stripped metadata is heal's own footprint, not a user edit",
        );
        let body = std::fs::read_to_string(&target).unwrap();
        assert!(body.contains("metadata:"), "metadata must be re-stamped");
    }

    #[test]
    fn skill_md_install_carries_frontmatter_metadata() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("skills");
        extract(&dest, ExtractMode::InstallSafe).unwrap();
        let body = std::fs::read_to_string(dest.join("heal-code-review/SKILL.md")).unwrap();
        assert!(body.contains("metadata:"));
        assert!(body.contains(&format!("heal-version: {}", bundled_version())));
        assert_eq!(
            read_installed_version(&body).as_deref(),
            Some(bundled_version().as_str())
        );
    }

    #[test]
    fn bundled_skill_names_lists_top_level_dirs() {
        let names = bundled_skill_names();
        assert!(names.iter().any(|n| n == "heal-cli"));
        assert!(names.iter().any(|n| n == "heal-config"));
        assert!(names.iter().any(|n| n == "heal-code-review"));
        assert!(names.iter().any(|n| n == "heal-code-patch"));
    }
}
