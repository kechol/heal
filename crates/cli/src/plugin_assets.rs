//! Build-time embedding of the Claude plugin tree (`plugins/heal/`).
//!
//! The plugin lives **outside** the cargo workspace by design (it is a
//! Claude Code asset, not Rust code). We embed it via `include_dir!` so the
//! resulting binary is self-contained and `heal skills install` can
//! materialize the tree on disk without network access.

use std::path::Path;

use anyhow::{Context, Result};
use include_dir::{include_dir, Dir, DirEntry};

pub static PLUGIN_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../plugins/heal");

/// Extract the embedded plugin tree under `dest`, creating directories as
/// needed. Existing files are skipped unless `overwrite` is true.
///
/// Note: `include_dir`'s built-in `Dir::extract` always overwrites and
/// can't track stats, so HEAL ships its own walker. Each embedded entry
/// stores its full path relative to the include root, which lets the
/// recursion stay flat — `dest.join(entry.path())` lands at the correct
/// nested location regardless of depth.
pub fn extract(dest: &Path, overwrite: bool) -> Result<ExtractStats> {
    std::fs::create_dir_all(dest)
        .with_context(|| format!("creating plugin dest dir {}", dest.display()))?;
    let mut stats = ExtractStats::default();
    extract_into(&PLUGIN_DIR, dest, overwrite, &mut stats)?;
    Ok(stats)
}

fn extract_into(
    dir: &Dir<'_>,
    dest: &Path,
    overwrite: bool,
    stats: &mut ExtractStats,
) -> Result<()> {
    for entry in dir.entries() {
        let target = dest.join(entry.path());
        match entry {
            DirEntry::Dir(child) => {
                std::fs::create_dir_all(&target)
                    .with_context(|| format!("mkdir {}", target.display()))?;
                extract_into(child, dest, overwrite, stats)?;
            }
            DirEntry::File(file) => {
                if target.exists() && !overwrite {
                    stats.skipped += 1;
                    continue;
                }
                std::fs::write(&target, file.contents())
                    .with_context(|| format!("writing {}", target.display()))?;
                #[cfg(unix)]
                if target.extension().is_some_and(|ext| ext == "sh") {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = std::fs::metadata(&target)?.permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(&target, perms)?;
                }
                stats.written += 1;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ExtractStats {
    pub written: usize,
    pub skipped: usize,
}
