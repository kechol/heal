//! What a semantic task is: a local, deterministic enumeration of
//! subjects plus one typed question per subject.
//!
//! A task never talks to the network. It reads the project (the same way
//! an observer does), decides which subjects are worth asking about, and
//! returns groups of questions that share one `state`. The runner handles
//! caching, packing, pricing, and sending; `Feature::lower` later turns
//! cached verdicts into Findings or decorations without a network call.

use std::path::Path;

use serde_json::Value;

use crate::core::config::{Config, SemanticConfig};
use crate::observer::shared::walk::ExcludeMatcher;
use crate::semantic::api::Question;
use crate::semantic::store::{content_hash, verdict_key};

/// Inputs every task can read.
pub struct TaskContext<'a> {
    pub project: &'a Path,
    pub config: &'a Config,
    /// Files whose content must never be sent (`[features.semantic].exclude`).
    pub exclude: ExcludeMatcher,
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
            focus: None,
            diff_range: None,
        })
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
}

/// One question and the cache key its answer is stored under.
#[derive(Debug, Clone)]
pub struct Item {
    pub key: String,
    pub question: Question,
}

/// Questions that share one `state`. The runner may split a group across
/// several requests, but never merges states.
#[derive(Debug, Clone)]
pub struct Group {
    pub state: Value,
    pub items: Vec<Item>,
}

pub trait Task: Sync {
    /// Stable id: the verdict file name, the `[features.semantic.tasks]`
    /// key, and a cache-key input.
    fn id(&self) -> &'static str;

    /// One line for `heal semantic ask --dry-run` and `--help` output.
    fn summary(&self) -> &'static str;

    /// Every piece of text sent to the model that is not the subject
    /// itself: instructions and criteria. Hashed into every cache key so a
    /// wording change re-asks instead of reusing stale verdicts.
    fn criteria_text(&self) -> String;

    #[must_use]
    fn criteria_hash(&self) -> String {
        content_hash(self.criteria_text().as_bytes())
    }

    /// Enumerate subjects locally and build their questions.
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>>;
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
