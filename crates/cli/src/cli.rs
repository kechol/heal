use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::commands;

#[derive(Debug, Parser)]
#[command(name = "heal", version, about = "Code health hook-driven harness", long_about = None)]
pub struct Cli {
    /// Project root (defaults to the current directory).
    #[arg(long, global = true)]
    pub project: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize `.heal/` and install hooks.
    Init {
        /// Overwrite an existing config.toml.
        #[arg(long)]
        force: bool,
    },
    /// Hook entrypoint invoked by git hooks and the Claude plugin.
    Hook {
        #[command(subcommand)]
        event: HookEvent,
    },
    /// Show metric summary and recent findings.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Browse `.heal/logs/` event entries (commit/edit/stop hook records).
    /// Commit entries hold metadata only — see `heal status` for the
    /// metric series persisted in `.heal/snapshots/`.
    Logs(LogsArgs),
    /// Launch Claude Code (`claude -p`) with the read-only check-* skills.
    Check,
    /// Manage the bundled Claude plugin.
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum HookEvent {
    /// Post-commit hook (git).
    Commit,
    /// PostToolUse(Edit|Write) hook (Claude plugin).
    Edit,
    /// Stop hook (Claude plugin).
    Stop,
}

impl HookEvent {
    /// Canonical event name embedded in `Event::event`. Co-located with the
    /// enum so adding a variant forces every dispatch site to update.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::Edit => "edit",
            Self::Stop => "stop",
        }
    }
}

#[derive(Debug, clap::Args)]
pub struct LogsArgs {
    /// Drop entries older than this RFC 3339 timestamp.
    #[arg(long)]
    pub since: Option<String>,
    /// Keep only entries whose `event` equals this name (e.g. `edit`).
    #[arg(long)]
    pub filter: Option<String>,
    /// Keep only the N most recent entries (after filtering).
    #[arg(long)]
    pub limit: Option<usize>,
    /// Emit raw JSONL instead of pretty text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum SkillsAction {
    /// Extract the bundled plugin into `.claude/plugins/heal/`.
    Install {
        #[arg(long)]
        force: bool,
    },
    /// Refresh plugin assets after a binary upgrade.
    Update,
    /// Show installed plugin status.
    Status,
    /// Remove the plugin from `.claude/plugins/heal/`.
    Uninstall,
}

impl Cli {
    pub fn run(self) -> Result<()> {
        let project = self
            .project
            .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
        match self.command {
            Command::Init { force } => commands::init::run(&project, force),
            Command::Hook { event } => commands::hook::run(&project, event),
            Command::Status { json } => commands::status::run(&project, json),
            Command::Logs(args) => commands::logs::run(&project, &args),
            Command::Check => commands::check::run(&project),
            Command::Skills { action } => commands::skills::run(&project, action),
        }
    }
}
