//! `heal semantic ask` — the one command that sends project content to
//! the Jev API (`[features.semantic]`). Everything it learns is written to
//! `.heal/semantic/verdicts/`; `heal status` reads that cache offline.

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};

use crate::core::config::load_from_project;
use crate::core::HealPaths;
use crate::semantic::client::{JevClient, UreqTransport};
use crate::semantic::credentials::{default_credentials_path, resolve};
use crate::semantic::runner::{self, AskOptions, AskReport};
use crate::semantic::store::VerdictStore;
use crate::semantic::task::{registry, Task, TaskContext};

/// Exit code when the run needs something the user must supply (a key,
/// an enabled feature) rather than a code change.
pub const SEMANTIC_SETUP_EXIT_CODE: i32 = 2;

#[allow(clippy::struct_excessive_bools)] // mirrors independent CLI flags
#[derive(Debug, Clone, Default)]
pub struct AskArgs {
    pub tasks: Vec<String>,
    pub dry_run: bool,
    pub refresh: bool,
    pub prune: bool,
    pub check: bool,
    pub json: bool,
    pub focus: Option<String>,
    pub diff: Option<String>,
}

pub fn run_ask(project: &Path, args: &AskArgs) -> Result<()> {
    let cfg = load_from_project(project)?;
    let semantic = &cfg.features.semantic;
    if !semantic.enabled {
        eprintln!(
            "heal semantic: [features.semantic] is disabled in .heal/config.toml.\n\
             Enabling it sends selected source and docs to the TypeSafe API \
             (https://docs.typesafe.ai/); set `enabled = true` under \
             [features.semantic] to opt in."
        );
        std::process::exit(SEMANTIC_SETUP_EXIT_CODE);
    }

    let all = registry();
    let selected = select_tasks(&all, &args.tasks, |id| semantic.task_enabled(id))?;

    let client = if args.dry_run {
        None
    } else {
        match build_client(&semantic.model) {
            Ok(c) => Some(c),
            Err(e) => setup_exit(&e.to_string()),
        }
    };

    if args.check {
        let Some(client) = client.as_ref() else {
            bail!("--check cannot be combined with --dry-run");
        };
        let models = match client.list_models() {
            Ok(m) => m,
            Err(e) => setup_exit(&format!("Jev API check failed: {e}")),
        };
        if !models.is_empty() && !models.iter().any(|m| m == &semantic.model) {
            setup_exit(&format!(
                "the key is valid but model `{}` is not available to it (available: {})",
                semantic.model,
                models.join(", ")
            ));
        }
        println!("Jev API reachable; model `{}` available.", semantic.model);
        return Ok(());
    }

    let focus = match args.focus.as_deref() {
        Some("-") => {
            Some(std::io::read_to_string(std::io::stdin()).context("reading --focus from stdin")?)
        }
        Some(path) => Some(
            std::fs::read_to_string(path)
                .with_context(|| format!("reading --focus file {path}"))?,
        ),
        None => None,
    };
    let paths = HealPaths::new(project);
    let (reports, base) = crate::observers::base_observation(project, &paths, &cfg);
    let mut ctx = TaskContext::new(project, &cfg)
        .map_err(anyhow::Error::msg)?
        .with_base(&reports, &base);
    ctx.focus = focus.as_deref();
    ctx.diff_range = args.diff.as_deref();

    let mut store = VerdictStore::new(paths.semantic_verdicts());
    let report = runner::run(
        &selected,
        &ctx,
        &mut store,
        client.as_ref(),
        AskOptions {
            dry_run: args.dry_run,
            refresh: args.refresh,
            prune: args.prune,
        },
    )?;
    if !report.dry_run {
        store.save().map_err(anyhow::Error::msg)?;
    }

    if args.json {
        super::emit_json(&report);
    } else {
        print_report(&report);
    }
    if let Some(fatal) = &report.fatal {
        eprintln!("heal semantic: stopped: {fatal}");
        std::process::exit(SEMANTIC_SETUP_EXIT_CODE);
    }
    Ok(())
}

/// Print `message` and exit with [`SEMANTIC_SETUP_EXIT_CODE`]: the run
/// needs something only the user can supply (a key, credit, a model).
fn setup_exit(message: &str) -> ! {
    eprintln!("heal semantic: {message}");
    std::process::exit(SEMANTIC_SETUP_EXIT_CODE);
}

fn select_tasks<'a>(
    all: &'a [Box<dyn Task>],
    requested: &[String],
    enabled: impl Fn(&str) -> bool,
) -> Result<Vec<&'a dyn Task>> {
    for id in requested {
        if !all.iter().any(|t| t.id() == id) {
            let known: Vec<&str> = all.iter().map(|t| t.id()).collect();
            bail!(
                "unknown task `{id}`; known tasks: {}",
                if known.is_empty() {
                    "(none)".to_owned()
                } else {
                    known.join(", ")
                }
            );
        }
    }
    Ok(all
        .iter()
        .filter(|t| {
            if requested.is_empty() {
                enabled(t.id()) && !t.on_demand()
            } else {
                requested.iter().any(|r| r == t.id())
            }
        })
        .map(AsRef::as_ref)
        .collect())
}

/// Resolve the key and build a client. Shared with `heal auth jev status`.
pub(crate) fn build_client(model: &str) -> Result<JevClient> {
    let file = default_credentials_path();
    let key = resolve(file.as_deref())
        .map_err(anyhow::Error::msg)?
        .ok_or_else(|| {
            anyhow!(
                "no Jev API key found. Export TYPESAFE_API_KEY, or run \
                 `heal auth jev set` to store one in your user config \
                 (never in .heal/). Keys: https://console.typesafe.ai/"
            )
        })?;
    let transport = UreqTransport::new().map_err(anyhow::Error::msg)?;
    Ok(JevClient::new(
        Arc::new(transport),
        key.key,
        model.to_owned(),
    ))
}

fn print_report(r: &AskReport) {
    let mode = if r.dry_run {
        "dry run (nothing sent)"
    } else {
        "sent"
    };
    println!("heal semantic ask — model {} — {mode}", r.model);
    if r.tasks.is_empty() {
        println!("  no enabled tasks");
    }
    for t in &r.tasks {
        println!(
            "  {:<20} subjects {:>5}  cached {:>5}  to ask {:>5}  requests {:>3}  est ${:.4}{}{}{}",
            t.task,
            t.subjects,
            t.cached,
            t.to_ask,
            t.requests,
            t.est_usd,
            if r.dry_run {
                String::new()
            } else {
                format!("  answered {}", t.answered)
            },
            if t.failed > 0 {
                format!("  failed {}", t.failed)
            } else {
                String::new()
            },
            if t.oversized_groups > 0 {
                format!("  oversized {}", t.oversized_groups)
            } else {
                String::new()
            },
        );
        if t.pruned > 0 {
            println!("  {:<20} pruned {} stale verdict(s)", "", t.pruned);
        }
    }
    if !r.dry_run {
        println!(
            "  total: {} request(s), {} input tokens, ${:.4}",
            r.requests_sent, r.input_tokens, r.usd
        );
    }
    if r.stopped_by_budget {
        println!("  stopped before exceeding [features.semantic].max_usd; re-run to continue from the cache");
    }
    for e in &r.errors {
        eprintln!("  error: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_task_is_rejected_and_empty_selection_uses_enabled() {
        let all: Vec<Box<dyn Task>> = Vec::new();
        assert!(select_tasks(&all, &["nope".to_owned()], |_| true).is_err());
        assert!(select_tasks(&all, &[], |_| true).unwrap().is_empty());
    }
}
