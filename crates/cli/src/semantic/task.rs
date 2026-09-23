//! What a semantic task is: a local, deterministic enumeration of
//! subjects plus one typed question per subject, and the rule that turns
//! cached answers back into Findings or decorations.
//!
//! A task never talks to the network. `plan` reads the project and the
//! base observation (observer reports + the Findings the ordinary
//! families produced) and returns groups of questions that share one
//! `state`. `heal semantic ask` sends what is not cached yet; `heal
//! status` calls the same `plan`, looks every key up in the verdict
//! cache, and hands the answers to `lower`. Because both sides run the
//! identical `plan`, the keys line up by construction.

use std::path::Path;

use serde_json::Value;

use crate::core::config::{Config, SemanticConfig};
use crate::core::finding::{Finding, SemanticNote};
use crate::observer::shared::walk::ExcludeMatcher;
use crate::observers::ObserverReports;
use crate::semantic::api::{Answer, Question};
use crate::semantic::store::{content_hash, verdict_key};

/// Inputs every task can read.
pub struct TaskContext<'a> {
    pub project: &'a Path,
    pub config: &'a Config,
    /// Files whose content must never be sent (`[features.semantic].exclude`).
    pub exclude: ExcludeMatcher,
    /// Observer output for the current tree. `None` only in unit tests of
    /// tasks that do not need it.
    pub reports: Option<&'a ObserverReports>,
    /// Findings of the ordinary families, already classified and
    /// hotspot-decorated. Tasks pick their subjects from these.
    pub findings: &'a [Finding],
    /// `heal semantic ask --focus`: a description of upcoming work.
    pub focus: Option<&'a str>,
    /// `heal semantic ask --diff <range>`: the change a verify task judges.
    pub diff_range: Option<&'a str>,
}

impl<'a> TaskContext<'a> {
    pub fn new(project: &'a Path, config: &'a Config) -> Result<Self, String> {
        let exclude = ExcludeMatcher::compile(project, &config.features.semantic.exclude)
            .map_err(|e| format!("[features.semantic].exclude: {e}"))?;
        Ok(Self {
            project,
            config,
            exclude,
            reports: None,
            findings: &[],
            focus: None,
            diff_range: None,
        })
    }

    #[must_use]
    pub fn with_base(mut self, reports: &'a ObserverReports, findings: &'a [Finding]) -> Self {
        self.reports = Some(reports);
        self.findings = findings;
        self
    }

    #[must_use]
    pub fn semantic(&self) -> &SemanticConfig {
        &self.config.features.semantic
    }

    /// True when `rel` (project-relative) may be sent to the API.
    #[must_use]
    pub fn may_send(&self, rel: &Path) -> bool {
        !self.exclude.is_excluded(rel, false)
    }

    /// Read a project-relative source file, or `None` when it is excluded
    /// from sending or unreadable.
    #[must_use]
    pub fn read_sendable(&self, rel: &Path) -> Option<String> {
        if !self.may_send(rel) {
            return None;
        }
        std::fs::read_to_string(self.project.join(rel)).ok()
    }

    /// Cache key for one question of `task` about `subject` under `state`.
    #[must_use]
    pub fn key(&self, task: &dyn Task, subject: &str, state: &str) -> String {
        verdict_key(
            task.id(),
            &task.criteria_hash(),
            &self.semantic().model,
            subject,
            state,
        )
    }

    /// The task's cutoff: the per-task override, else `default`.
    #[must_use]
    pub fn cutoff(&self, task: &dyn Task, default: f64) -> f64 {
        self.semantic().cutoff(task.id()).unwrap_or(default)
    }
}

/// One question, the cache key its answer is stored under, and whatever
/// the task needs later to lower the answer (file, symbol, lines, the
/// finding id it decorates). `meta` never reaches the network.
#[derive(Debug, Clone)]
pub struct Item {
    pub key: String,
    pub question: Question,
    pub meta: Value,
}

/// Questions that share one `state`. The runner may split a group across
/// several requests, but never merges states.
#[derive(Debug, Clone)]
pub struct Group {
    pub state: Value,
    pub items: Vec<Item>,
}

/// An item with its cached answer, if any.
pub struct Answered<'a> {
    pub item: &'a Item,
    pub answer: Option<&'a Answer>,
}

/// What a task contributes to `heal status`.
#[derive(Debug, Default)]
pub struct Lowered {
    /// New Findings (Severity already assigned by the task).
    pub findings: Vec<Finding>,
    /// Decorations on existing Findings: `(finding id, note name, note)`.
    pub notes: Vec<(String, String, SemanticNote)>,
}

pub trait Task: Sync {
    /// Stable id: the verdict file name, the `[features.semantic.tasks]`
    /// key, and a cache-key input.
    fn id(&self) -> &'static str;

    /// One line for `heal semantic ask --dry-run` output and docs.
    fn summary(&self) -> &'static str;

    /// Every piece of text sent to the model that is not the subject
    /// itself: instructions and criteria. Hashed into every cache key so a
    /// wording change re-asks instead of reusing stale verdicts.
    fn criteria_text(&self) -> String;

    #[must_use]
    fn criteria_hash(&self) -> String {
        content_hash(self.criteria_text().as_bytes())
    }

    /// Only asked on explicit request (`--task <id>`), never as part of
    /// a plain `heal semantic ask` (e.g. verify tasks that judge a diff).
    fn on_demand(&self) -> bool {
        false
    }

    /// Enumerate subjects locally and build their questions.
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>>;

    /// Turn cached answers into Findings and decorations. Items without
    /// an answer have not been asked yet and must be skipped.
    fn lower(&self, _ctx: &TaskContext<'_>, _answered: &[Answered<'_>]) -> Lowered {
        Lowered::default()
    }
}

/// Every task HEAL ships, in a stable order. Ids must match
/// [`crate::core::config::SEMANTIC_TASK_IDS`].
#[must_use]
pub fn registry() -> Vec<Box<dyn Task>> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_match_config_allowlist() {
        let ids: Vec<&str> = registry().iter().map(|t| t.id()).collect();
        assert_eq!(ids, crate::core::config::SEMANTIC_TASK_IDS);
    }
}
