use std::path::Path;

use anyhow::Result;

// `Result<()>` is intentional: keeps the dispatcher signature uniform across
// command modules; the v0.1 stub will gain real fallible work in upcoming
// observer-wiring items.
#[allow(clippy::unnecessary_wraps)]
pub fn run(_project: &Path) -> Result<()> {
    println!("`heal check` is not yet implemented; coming in v0.1 once observer wiring lands.");
    Ok(())
}
