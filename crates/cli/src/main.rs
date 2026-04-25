use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;
mod plugin_assets;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    cli.run()
}
