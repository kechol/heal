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
        /// Assume "yes" for the Claude-skills install prompt
        /// (extracts the bundled plugin without asking).
        #[arg(long, short = 'y', conflicts_with = "no_skills")]
        yes: bool,
        /// Skip the Claude-skills install prompt entirely. Use when you
        /// don't have Claude Code installed, or for CI invocations.
        #[arg(long)]
        no_skills: bool,
        /// Emit a machine-readable JSON summary of the init outcome
        /// instead of the human-readable text. Stable contract for
        /// scripts and the `heal-config` skill.
        #[arg(long)]
        json: bool,
    },
    /// Hook entrypoint invoked by git hooks and Claude Code's
    /// `settings.json` hook commands. No-ops silently when the project
    /// has no `.heal/` directory.
    Hook {
        #[command(subcommand)]
        event: HookEvent,
    },
    /// Per-metric summary plus the delta since the previous snapshot.
    Metrics {
        #[arg(long)]
        json: bool,
        /// Restrict output to a single metric. Used by the
        /// `/heal-code-review` skill under `.claude/skills/` when
        /// narrowing focus.
        #[arg(long, value_enum)]
        metric: Option<MetricKind>,
        /// Restrict every observer to files under `<path>` (relative
        /// to the project root). Matches the `[[project.workspaces]]`
        /// path of one declared workspace; segment-wise prefix so
        /// `pkg/web` does not match `pkg/webapp`. Each observer scopes
        /// itself: Loc walks only that sub-tree, walk-based observers
        /// drop out-of-workspace files, and git-based observers
        /// recompute `commits_considered` against the in-workspace
        /// universe so lift / churn totals stay consistent.
        #[arg(long, value_name = "PATH")]
        workspace: Option<std::path::PathBuf>,
    },
    /// Render the cached `CheckRecord` from `.heal/findings/latest.json`
    /// — Critical / High view by default. Runs a fresh scan only when
    /// the cache is missing; pass `--refresh` to force a rescan and
    /// overwrite the cache. The single source of truth that
    /// `/heal-code-patch` (Claude side) and `heal diff` consume.
    Status(StatusArgs),
    /// Diff the current findings against a cached `CheckRecord` whose
    /// `head_sha` matches the resolved git ref (default: `HEAD`).
    /// Outputs Resolved / Regressed / Improved / New / Unchanged
    /// buckets — like `git diff`, but for the TODO list.
    Diff(DiffArgs),
    /// Record a finding as resolved by a commit — called by
    /// `/heal-code-patch` after each fix commit. The next
    /// `heal status --refresh` either retires the entry (genuinely
    /// fixed) or moves it to `regressed.jsonl` (re-detected). Hidden
    /// from `--help` because no human ever invokes this directly;
    /// implemented as an upsert into `.heal/findings/fixed.json`.
    #[command(hide = true)]
    MarkFixed {
        /// `Finding.id` from `heal status --json` output.
        #[arg(long, value_name = "ID")]
        finding_id: String,
        /// SHA of the commit that resolved the finding.
        #[arg(long, value_name = "SHA")]
        commit_sha: String,
        /// Emit a JSON summary of the recorded fix entry.
        #[arg(long)]
        json: bool,
    },
    /// Manage the bundled Claude skill set under `.claude/skills/`.
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },
    /// Calibrate codebase-relative Severity thresholds. Default
    /// behaviour:
    ///   * `calibration.toml` missing → run a fresh scan and write it.
    ///   * `calibration.toml` present → print the freshness summary and
    ///     surface `--force` as the way to refresh. The `heal-config`
    ///     skill is responsible for deciding when to suggest a
    ///     recalibration; HEAL itself never auto-fires.
    Calibrate {
        /// Force a fresh scan and overwrite `.heal/calibration.toml`
        /// even when one already exists.
        #[arg(long)]
        force: bool,
        /// Emit a JSON summary instead of the human-readable text.
        /// Stable contract for the `heal-config` skill and CI scripts.
        #[arg(long)]
        json: bool,
    },
}

/// Metric filter for `heal metrics --metric`. clap renders these in
/// kebab-case for the CLI flag (e.g. `--metric change-coupling`), and
/// [`Self::json_key`] returns the `snake_case` form that matches the
/// JSON object key under which the same metric's data is keyed
/// (`change_coupling`). The two forms are deliberately distinct: the
/// CLI follows shell convention, the JSON follows the rest of the
/// payload's `snake_case` keys, so a skill can do `payload[payload.metric]`
/// without translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum MetricKind {
    Loc,
    Complexity,
    Churn,
    ChangeCoupling,
    Duplication,
    Hotspot,
    Lcom,
}

impl MetricKind {
    /// JSON object key matching this metric's data section. Identical
    /// to the field names used in `MetricsConfig` so skills can index
    /// `payload[payload.metric]`.
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

/// Filter for `heal status --metric`. Distinct from [`MetricKind`]
/// because `complexity` here is an alias that selects both `ccn` and
/// `cognitive` findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum FindingMetric {
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

impl FindingMetric {
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
    /// Claude Code PostToolUse(Edit|Write|MultiEdit) hook. No-op kept
    /// for back-compat with stale `settings.json` registrations.
    Edit,
    /// Claude Code Stop hook. No-op kept for back-compat with stale
    /// `settings.json` registrations.
    Stop,
}

#[derive(Debug, clap::Args)]
#[allow(clippy::struct_excessive_bools)] // every flag is independent CLI surface
pub struct StatusArgs {
    /// Restrict the rendered list to one metric (or one metric family —
    /// `complexity` covers both CCN and Cognitive).
    #[arg(long, value_enum)]
    pub metric: Option<FindingMetric>,
    /// Restrict to findings inside one declared
    /// `[[project.workspaces]]` entry. The value is the workspace's
    /// `path` (the same string `Finding.workspace` carries).
    #[arg(long, value_name = "PATH")]
    pub workspace: Option<String>,
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
    /// `.heal/findings/latest.json` — stable contract for skills and CI.
    #[arg(long)]
    pub json: bool,
    /// Re-scan the project and overwrite `.heal/findings/latest.json`
    /// instead of reading the cached record. Without this, a present
    /// cache is reused as-is; only a missing cache triggers a scan.
    #[arg(long)]
    pub refresh: bool,
    /// Cap each Severity bucket at the N worst findings.
    #[arg(long, value_name = "N")]
    pub top: Option<usize>,
}

/// Args for `heal diff`. The positional `revspec` accepts anything
/// `git rev-parse` understands — `HEAD` (default), `main`, `v0.2.1`,
/// `HEAD~3`, or a partial / full SHA.
#[derive(Debug, clap::Args)]
pub struct DiffArgs {
    /// Git revision to diff against. Resolves against the local repo;
    /// the matching `CheckRecord` must already exist in `.heal/findings/`.
    #[arg(value_name = "GIT_REF", default_value = "HEAD")]
    pub revspec: String,
    /// Restrict to findings inside one declared
    /// `[[project.workspaces]]` entry. The value is the workspace's
    /// `path` (the same string `Finding.workspace` carries).
    #[arg(long, value_name = "PATH")]
    pub workspace: Option<String>,
    /// Show the Improved / Unchanged buckets in addition to Resolved /
    /// Regressed / New. (Distinct from `heal status --all`, which
    /// surfaces lower Severity tiers; this flag has no effect on
    /// Severity filtering.)
    #[arg(long)]
    pub all: bool,
    /// Emit the diff as JSON on stdout. Stable contract for skills.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum SkillsAction {
    /// Extract the bundled skills into `<project>/.claude/skills/` and
    /// merge HEAL's hook commands into `<project>/.claude/settings.json`.
    Install {
        /// Overwrite existing skill files even if they were edited locally.
        #[arg(long)]
        force: bool,
        /// Emit a JSON summary of the install outcome.
        #[arg(long)]
        json: bool,
    },
    /// Refresh skill files after a binary upgrade. Skips files the user
    /// has edited locally; pass `--force` to overwrite them too.
    Update {
        #[arg(long)]
        force: bool,
        /// Emit a JSON summary of the update outcome.
        #[arg(long)]
        json: bool,
    },
    /// Show installed skill version, bundled version, and any drift.
    Status {
        /// Emit a JSON view of the install status (versions, drift list).
        #[arg(long)]
        json: bool,
    },
    /// Remove HEAL's skills from `.claude/skills/` and its hook
    /// commands from `.claude/settings.json`.
    Uninstall {
        /// Emit a JSON summary of what was removed.
        #[arg(long)]
        json: bool,
    },
}

impl Cli {
    pub fn run(self) -> Result<()> {
        let project = self
            .project
            .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
        match self.command {
            Command::Init {
                force,
                yes,
                no_skills,
                json,
            } => commands::init::run(&project, force, yes, no_skills, json),
            Command::Hook { event } => commands::hook::run(&project, event),
            Command::Metrics {
                json,
                metric,
                workspace,
            } => commands::metrics::run(&project, json, metric, workspace.as_deref()),
            Command::Status(args) => commands::status::run(&project, &args),
            Command::Diff(args) => commands::diff::run(
                &project,
                &args.revspec,
                args.workspace.as_deref(),
                args.all,
                args.json,
            ),
            Command::MarkFixed {
                finding_id,
                commit_sha,
                json,
            } => commands::mark_fixed::run(&project, &finding_id, &commit_sha, json),
            Command::Skills { action } => commands::skills::run(&project, action),
            Command::Calibrate { force, json } => commands::calibrate::run(&project, force, json),
        }
    }
}
