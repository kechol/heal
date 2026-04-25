use std::path::Path;

use anyhow::{Context, Result};
use heal_core::{config::Config, HealPaths};

pub fn run(project: &Path, force: bool) -> Result<()> {
    let paths = HealPaths::new(project);
    paths
        .ensure()
        .with_context(|| format!("creating {}", paths.root().display()))?;

    let cfg_path = paths.config();
    if cfg_path.exists() && !force {
        println!(
            "config already exists: {} (use --force to overwrite)",
            cfg_path.display()
        );
    } else {
        let cfg = Config::recommended_for_solo();
        cfg.save(&cfg_path)?;
        println!("wrote {}", cfg_path.display());
    }

    println!("initialized .heal/ at {}", paths.root().display());
    println!("next steps:");
    println!("  1. heal skills install   # install Claude plugin");
    println!("  2. heal status           # see current findings");
    Ok(())
}
