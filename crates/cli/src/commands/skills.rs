use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::cli::SkillsAction;
use crate::plugin_assets;

pub fn run(project: &Path, action: SkillsAction) -> Result<()> {
    let dest = plugin_dest(project);
    match action {
        SkillsAction::Install { force } => install(&dest, force),
        SkillsAction::Update => install(&dest, true),
        SkillsAction::Status => {
            status(&dest);
            Ok(())
        }
        SkillsAction::Uninstall => uninstall(&dest),
    }
}

fn plugin_dest(project: &Path) -> PathBuf {
    project.join(".claude").join("plugins").join("heal")
}

fn install(dest: &Path, overwrite: bool) -> Result<()> {
    let stats = plugin_assets::extract(dest, overwrite)?;
    println!(
        "plugin extracted to {}: {} written, {} skipped",
        dest.display(),
        stats.written,
        stats.skipped
    );
    Ok(())
}

fn status(dest: &Path) {
    if dest.exists() {
        let manifest = dest.join("plugin.json");
        if manifest.exists() {
            println!("plugin installed at {}", dest.display());
            println!("  manifest: {}", manifest.display());
        } else {
            println!(
                "plugin directory exists but plugin.json is missing: {}",
                dest.display()
            );
        }
    } else {
        println!("plugin not installed (run `heal skills install`)");
    }
}

fn uninstall(dest: &Path) -> Result<()> {
    if !dest.exists() {
        println!("plugin not installed; nothing to do");
        return Ok(());
    }
    std::fs::remove_dir_all(dest).with_context(|| format!("removing {}", dest.display()))?;
    println!("removed {}", dest.display());
    Ok(())
}
