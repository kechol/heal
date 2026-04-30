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
    /// Render the cached `CheckRecord` from `.heal/checks/latest.json`
    /// — Critical / High view by default. Runs a fresh scan only when
    /// the cache is missing; pass `--refresh` to force a rescan and
    /// overwrite the cache. The single source of truth that
    /// `/heal-fix` (Claude side) and `heal cache *` (read-only) consume.
    Check(CheckArgs),
    /// Inspect the `.heal/checks/` cache: enumerate records (`log`),
    /// render one (`show`), diff two (`diff`), or claim a commit as
    /// fixing a finding (`mark-fixed`, used by `/heal-fix`).
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
    /// Manage the bundled Claude plugin.
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },
    /// Calibrate codebase-relative Severity thresholds. Default
    /// behaviour:
    ///   * `calibration.toml` missing → run a fresh scan and write it.
    ///   * `calibration.toml` present → evaluate auto-detect drift
    ///     triggers (no write) and surface `--force` as the way to
    ///     refresh.
    Calibrate {
        /// Force a fresh scan and overwrite `.heal/calibration.toml`
        /// even when one already exists.
        #[arg(long)]
        force: bool,
    },
    /// Compact `.heal/{snapshots,logs,checks}/` segments. Files older
    /// than 90 days are gzipped in place; files older than 365 days
    /// are deleted. Idempotent — also called automatically from
    /// `heal hook commit`, so manual runs are mostly for diagnostics.
    Compact {
        /// Print one line per touched file instead of just the summary.
        #[arg(long)]
        verbose: bool,
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
    Lcom,
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
            Self::Lcom => "lcom",
        }
    }
}

/// Filter for `heal check --metric`. Distinct from [`StatusMetric`]
/// because `complexity` here is an alias that selects both `ccn` and
/// `cognitive` findings (TODO §「heal status の延長で metric 指定」).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CheckMetric {
    Ccn,
    Cognitive,
    /// CCN + Cognitive together.
    Complexity,
    Duplication,
    /// `change_coupling` symmetric pairs.
    Coupling,
    Hotspot,
    /// `lcom` — class-level Lack of Cohesion of Methods.
    Lcom,
}

impl CheckMetric {
    /// Does a `Finding.metric` string belong to this filter? Used by
    /// the renderer when narrowing the displayed list.
    #[must_use]
    pub fn matches(self, metric: &str) -> bool {
        match self {
            Self::Ccn => metric == "ccn",
            Self::Cognitive => metric == "cognitive",
            Self::Complexity => matches!(metric, "ccn" | "cognitive"),
            Self::Duplication => metric == "duplication",
            Self::Coupling => matches!(metric, "change_coupling" | "change_coupling.symmetric"),
            Self::Hotspot => metric == "hotspot",
            Self::Lcom => metric == "lcom",
        }
    }
}

/// CLI-side mirror of [`crate::core::severity::Severity`] so clap's
/// `value_enum` can render the four labels without leaking SGR colour
/// codes into the help text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SeverityFilter {
    Critical,
    High,
    Medium,
    Ok,
}

impl SeverityFilter {
    #[must_use]
    pub fn into_severity(self) -> crate::core::severity::Severity {
        use crate::core::severity::Severity;
        match self {
            Self::Critical => Severity::Critical,
            Self::High => Severity::High,
            Self::Medium => Severity::Medium,
            Self::Ok => Severity::Ok,
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
#[allow(clippy::struct_excessive_bools)] // every flag is independent CLI surface
pub struct CheckArgs {
    /// Restrict the rendered list to one metric (or one metric family —
    /// `complexity` covers both CCN and Cognitive).
    #[arg(long, value_enum)]
    pub metric: Option<CheckMetric>,
    /// Restrict to findings under a path prefix (e.g.
    /// `--feature src/payments`). Matched against `Finding.location.file`.
    #[arg(long)]
    pub feature: Option<String>,
    /// Severity floor — show only this level. Combine with `--all` to
    /// also surface lower severities below it.
    #[arg(long, value_enum)]
    pub severity: Option<SeverityFilter>,
    /// Show every Severity tier (Medium / Ok included) plus the
    /// low-Severity hotspot section. Without this, only Critical /
    /// High render (with a "(N) hidden — pass `--all`" footer when
    /// there are more).
    #[arg(long)]
    pub all: bool,
    /// Emit the `CheckRecord` payload as JSON on stdout. Same shape as
    /// `.heal/checks/latest.json` — stable contract for skills and CI.
    #[arg(long)]
    pub json: bool,
    /// Re-scan the project and overwrite `.heal/checks/latest.json`
    /// instead of reading the cached record. Without this, a present
    /// cache is reused as-is; only a missing cache triggers a scan.
    #[arg(long)]
    pub refresh: bool,
    /// Cap each Severity bucket at the N worst findings.
    #[arg(long, value_name = "N")]
    pub top: Option<usize>,
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

#[derive(Debug, Subcommand)]
pub enum CacheAction {
    /// List `CheckRecord`s newest-first (`check_id`, `started_at`,
    /// `head_sha`, finding count, severity tally).
    Log {
        #[arg(long)]
        json: bool,
        /// Cap at the N most recent records.
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
    },
    /// Append a `FixedFinding` to `.heal/checks/fixed.jsonl`. Called by
    /// `/heal-fix` (or any skill that commits a fix) so the next
    /// `heal check` can warn if the same finding re-appears.
    MarkFixed {
        /// `Finding.id` from `heal check --json` output.
        #[arg(long, value_name = "ID")]
        finding_id: String,
        /// SHA of the commit that resolved the finding.
        #[arg(long, value_name = "SHA")]
        commit_sha: String,
    },
    /// Render one `CheckRecord` by its ULID. **Unstable**: the human
    /// view may change. For a stable contract use `--json` (same shape
    /// as `heal check --json`).
    Show {
        check_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Compare two `CheckRecord`s — Resolved / Regressed / Improved /
    /// New / Unchanged buckets, plus a progress percentage. With no
    /// arguments, diffs the prior cache record against the latest. With
    /// `--worktree`, scans the live project (no cache write) and diffs
    /// it against the latest cache record so a half-finished session
    /// can verify progress before committing.
    Diff {
        /// Older `check_id` for the diff. Defaults to the second-most-
        /// recent record.
        #[arg(value_name = "FROM")]
        from: Option<String>,
        /// Newer `check_id`. Defaults to the most-recent record.
        #[arg(value_name = "TO")]
        to: Option<String>,
        /// Re-scan the working tree instead of reading `to` from the
        /// cache. Conflicts with `to`.
        #[arg(long, conflicts_with = "to")]
        worktree: bool,
        /// Show the Improved / Unchanged buckets too.
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
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
            Command::Cache { action } => commands::cache::run(&project, action),
            Command::Skills { action } => commands::skills::run(&project, action),
            Command::Calibrate { force } => commands::calibrate::run(&project, force),
            Command::Compact { verbose } => commands::compact::run(&project, verbose),
        }
    }
}
