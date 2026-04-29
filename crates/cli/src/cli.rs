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
    /// Anything after `--` is forwarded verbatim to `claude` (e.g.
    /// `heal check hotspots -- --model claude-opus-4-7`).
    Check(CheckArgs),
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

/// Output format hint passed to `claude -p` via the prompt body. The
/// model still decides on the final shape, but the hint nudges it
/// toward the renderer the user will actually see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    /// TTY → plain; pipe → markdown.
    Auto,
    /// Strip markdown affordances (`**bold**`, `# headers`, nested
    /// bullets) — terminal-friendly.
    Plain,
    /// Let the model use its default markdown.
    Markdown,
}

/// Read-only Claude skill to invoke from `heal check`. The variants map
/// 1:1 to the bundled `plugins/heal/skills/check-*` directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CheckSkill {
    Overview,
    Hotspots,
    Complexity,
    Duplication,
    Coupling,
}

impl CheckSkill {
    /// Full skill identifier as it appears on disk and in `plugin.json`.
    #[must_use]
    pub fn skill_name(self) -> &'static str {
        match self {
            Self::Overview => "check-overview",
            Self::Hotspots => "check-hotspots",
            Self::Complexity => "check-complexity",
            Self::Duplication => "check-duplication",
            Self::Coupling => "check-coupling",
        }
    }

    /// Short name used as the CLI argument (`heal check hotspots`).
    #[must_use]
    pub fn short_name(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Hotspots => "hotspots",
            Self::Complexity => "complexity",
            Self::Duplication => "duplication",
            Self::Coupling => "coupling",
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
pub struct CheckArgs {
    /// Which check-* skill to run. Defaults to the overview hub.
    #[arg(value_enum, default_value_t = CheckSkill::Overview)]
    pub skill: CheckSkill,
    /// Output format for the response body. `auto` (default) probes
    /// stdout: a TTY gets `plain`, a pipe gets `markdown`. `plain`
    /// strips markdown affordances so headings/bold don't show as
    /// raw `**` / `#` in a terminal; `markdown` lets the model use
    /// its default formatting.
    #[arg(long, value_enum, default_value_t = OutputFormat::Auto)]
    pub format: OutputFormat,
    /// Suppress per-tool progress lines on stderr. The final synthesis
    /// still prints to stdout.
    #[arg(long, conflicts_with = "raw")]
    pub quiet: bool,
    /// Forward `claude -p` output verbatim instead of parsing
    /// stream-json into progress lines. Useful for piping to your own
    /// parser or for debugging.
    #[arg(long, conflicts_with = "quiet")]
    pub raw: bool,
    /// Pass-through arguments to the underlying `claude` invocation.
    /// e.g. `heal check hotspots -- --model claude-haiku-4-5 --effort low`.
    #[arg(last = true, allow_hyphen_values = true)]
    pub claude_args: Vec<String>,
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
            Command::Check(args) => commands::check::run(&project, &args),
            Command::Skills { action } => commands::skills::run(&project, action),
        }
    }
}
