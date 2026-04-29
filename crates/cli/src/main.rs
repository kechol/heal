use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    heal_cli::cli::Cli::parse().run()
}
