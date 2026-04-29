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
        /// Restrict output to a single metric. Drives the per-metric
        /// `check-*` skills under `.claude/plugins/heal/`.
        #[arg(long, value_enum)]
        metric: Option<StatusMetric>,
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

/// Metric filter for `heal status --metric`. clap renders these in
/// kebab-case for the CLI flag (e.g. `--metric change-coupling`), and
/// [`Self::json_key`] returns the `snake_case` form that matches the
/// JSON object key under which the same metric's data is keyed
/// (`change_coupling`). The two forms are deliberately distinct: the
/// CLI follows shell convention, the JSON follows the rest of the
/// payload's `snake_case` keys, so a skill can do `payload[payload.metric]`
/// without translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum StatusMetric {
    Loc,
    Complexity,
    Churn,
    ChangeCoupling,
    Duplication,
    Hotspot,
}

impl StatusMetric {
    /// JSON object key matching this metric's data section. Identical
    /// to the field names used in `MetricsConfig` and `SnapshotDelta`,
    /// so skills can index `payload[payload.metric]`.
    #[must_use]
    pub fn json_key(self) -> &'static str {
        match self {
            Self::Loc => "loc",
            Self::Complexity => "complexity",
            Self::Churn => "churn",
            Self::ChangeCoupling => "change_coupling",
            Self::Duplication => "duplication",
            Self::Hotspot => "hotspot",
        }
    }
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum HookEvent {
    /// Post-commit hook (git).
    Commit,
    /// PostToolUse(Edit|Write|MultiEdit) hook (Claude plugin).
    Edit,
    /// Stop hook (Claude plugin) — log only, no nudge.
    Stop,
    /// `SessionStart` hook (Claude plugin) — emits the cool-down-aware nudge.
    SessionStart,
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
            Self::SessionStart => "session-start",
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
        /// Overwrite existing assets even if they were edited locally.
        #[arg(long)]
        force: bool,
    },
    /// Refresh plugin assets after a binary upgrade. Skips files the user
    /// has edited locally; pass `--force` to overwrite them too.
    Update {
        #[arg(long)]
        force: bool,
    },
    /// Show installed plugin version, bundled version, and any drift.
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
            Command::Status { json, metric } => commands::status::run(&project, json, metric),
            Command::Logs(args) => commands::logs::run(&project, &args),
            Command::Check => commands::check::run(&project),
            Command::Skills { action } => commands::skills::run(&project, action),
        }
    }
}
