use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;
mod finding;
mod observers;
mod plugin_assets;
mod snapshot;
#[cfg(test)]
mod test_support;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    cli.run()
}
