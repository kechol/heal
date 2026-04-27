use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;
mod observers;
mod plugin_assets;
mod snapshot;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    cli.run()
}
