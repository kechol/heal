//! Q7 `commit_intent`: classify each commit in the churn window as a fix,
//! a feature, a refactor, … and derive a per-file `fix_ratio`.
//!
//! Files where bug fixes concentrate keep attracting defects — a pattern
//! reported repeatedly in defect-prediction research (e.g. Kamei et al.,
//! "A Large-Scale Empirical Study of Just-in-Time Quality Assurance", IEEE
//! TSE 2013). Conventional-Commit prefixes carry the same signal, but only
//! in repositories that use them; asking Jev recovers it everywhere.
//!
//! A commit never changes, so the verdict key depends only on the commit's
//! message and file list: every commit is asked once, ever.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use git2::Repository;
use serde_json::{json, Value};

use crate::core::finding::SemanticNote;
use crate::feature::Family;
use crate::observer::shared::git::collect_history;
use crate::semantic::api::{Answer, Question};
use crate::semantic::task::{Answered, Group, Item, Lowered, Task, TaskContext};

pub struct CommitIntent;

/// Newest commits considered. Bounds the first run on a long history.
const MAX_COMMITS: usize = 1000;
/// Files listed per commit in the question.
const MAX_FILES: usize = 40;
/// Message characters sent per commit.
const MAX_MESSAGE_CHARS: usize = 2000;

pub const LABELS: [(&str, &str); 6] = [
    (
        "fix",
        "Corrects behaviour that was wrong: a bug, crash, regression, wrong output, or security flaw.",
    ),
    (
        "feature",
        "Adds behaviour or a capability that users or callers can observe.",
    ),
    (
        "refactor",
        "Restructures code without changing observable behaviour: renames, extractions, moves, cleanups.",
    ),
    ("test", "Adds or changes tests only."),
    ("docs", "Changes documentation or comments only."),
    (
        "chore",
        "Build, dependencies, CI, formatting, releases, or other maintenance.",
    ),
];

const INSTRUCTIONS: &str =
    "Classify the main intent of this git commit from its message and the files it changed.";
const STATE: &str = "Commits from one software repository. Each question describes one commit.";

impl CommitIntent {
    fn criteria() -> BTreeMap<String, Value> {
        LABELS
            .iter()
            .map(|(k, v)| ((*k).to_owned(), json!(v)))
            .collect()
    }
}

impl Task for CommitIntent {
    fn id(&self) -> &'static str {
        "commit_intent"
    }

    fn summary(&self) -> &'static str {
        "classify each recent commit (fix / feature / refactor / …) to weight bug-fix churn"
    }

    fn criteria_text(&self) -> String {
        let mut s = format!("{INSTRUCTIONS}\n{STATE}\n");
        for (k, v) in LABELS {
            let _ = writeln!(s, "{k}: {v}");
        }
        s
    }

    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let mut excluded = ctx.config.exclude_lines();
        excluded.extend(ctx.semantic().exclude.iter().cloned());
        let history = collect_history(
            ctx.project,
            ctx.config.git.since_days,
            &excluded,
            None,
            true,
        );
        let Ok(repo) = Repository::discover(ctx.project) else {
            return Ok(Vec::new());
        };
        let mut items = Vec::new();
        for hc in history.commits.iter().take(MAX_COMMITS) {
            if hc.paths.is_empty() {
                continue;
            }
            let Ok(commit) = repo.find_commit(hc.oid) else {
                continue;
            };
            let message: String = commit
                .message()
                .unwrap_or("")
                .trim()
                .chars()
                .take(MAX_MESSAGE_CHARS)
                .collect();
            let mut files = String::new();
            for (path, ins, del) in hc.line_stats.iter().take(MAX_FILES) {
                let _ = writeln!(files, "{} +{ins} -{del}", path.display());
            }
            if hc.line_stats.is_empty() {
                for path in hc.paths.iter().take(MAX_FILES) {
                    let _ = writeln!(files, "{}", path.display());
                }
            }
            if hc.paths.len() > MAX_FILES {
                let _ = writeln!(files, "… and {} more files", hc.paths.len() - MAX_FILES);
            }
            let subject = format!("Message:\n{message}\n\nFiles changed:\n{files}");
            let paths: Vec<String> = hc
                .paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            items.push(Item {
                key: ctx.key(self, &subject, STATE),
                question: Question::Choice {
                    instructions: json!(format!("{INSTRUCTIONS}\n\n{subject}")),
                    criteria: Self::criteria(),
                },
                meta: json!({ "sha": hc.oid.to_string(), "paths": paths }),
            });
        }
        if items.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![Group {
            state: json!(STATE),
            items,
        }])
    }

    fn lower(&self, ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        let cutoff = ctx.cutoff(self, 0.5);
        // path → (fix commits, answered commits, confidence sum)
        let mut per_file: BTreeMap<String, (u32, u32, f64)> = BTreeMap::new();
        for a in answered {
            let Some(Answer::Choice {
                choice,
                probabilities,
                confidence,
            }) = a.answer
            else {
                continue;
            };
            let is_fix =
                choice == "fix" && probabilities.get("fix").copied().unwrap_or(0.0) >= cutoff;
            let paths: BTreeSet<&str> = a.item.meta["paths"]
                .as_array()
                .map(|v| v.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            for p in paths {
                let e = per_file.entry(p.to_owned()).or_default();
                e.1 += 1;
                e.2 += confidence;
                if is_fix {
                    e.0 += 1;
                }
            }
        }
        let mut lowered = Lowered::default();
        for f in ctx.findings {
            if Family::for_metric(&f.metric) != Family::Code {
                continue;
            }
            let Some(&(fixes, total, conf)) =
                per_file.get(&f.location.file.to_string_lossy().into_owned())
            else {
                continue;
            };
            if total == 0 {
                continue;
            }
            lowered.notes.push((
                f.id.clone(),
                "fix_ratio".to_owned(),
                SemanticNote {
                    label: "fix".to_owned(),
                    p: f64::from(fixes) / f64::from(total),
                    confidence: conf / f64::from(total),
                    lines: Vec::new(),
                    detail: Some(format!("{fixes} of {total} recent commits were bug fixes")),
                },
            ));
        }
        lowered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;
    use crate::core::finding::{Finding, Location};
    use crate::semantic::tasks::testing::{choice, instructions, lower_with};
    use crate::test_support::{commit, init_repo};
    use std::path::PathBuf;

    #[test]
    fn fix_ratio_counts_fix_commits_per_file() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "a.rs", "fn a() {}\n", "t@e.x", "feat: add a");
        commit(
            dir.path(),
            "a.rs",
            "fn a() { 1; }\n",
            "t@e.x",
            "fix: crash in a",
        );
        commit(dir.path(), "b.rs", "fn b() {}\n", "t@e.x", "fix: b");
        let cfg = Config::default();
        let findings = vec![Finding::new(
            "ccn",
            Location {
                file: PathBuf::from("a.rs"),
                line: Some(1),
                symbol: Some("a".into()),
            },
            "ccn".into(),
            "a",
        )];
        let reports = crate::observers::ObserverReports::default();
        let ctx = TaskContext::new(dir.path(), &cfg)
            .unwrap()
            .with_base(&reports, &findings);
        let (items, lowered) = lower_with(&CommitIntent, &ctx, |item| {
            let text = instructions(item);
            Some(if text.contains("fix:") {
                choice("fix", 0.9)
            } else {
                choice("feature", 0.8)
            })
        });
        assert_eq!(items.len(), 3);
        assert_eq!(lowered.notes.len(), 1);
        let (id, name, note) = &lowered.notes[0];
        assert_eq!(
            (id.as_str(), name.as_str()),
            (findings[0].id.as_str(), "fix_ratio")
        );
        assert!((note.p - 0.5).abs() < 1e-9);
        assert_eq!(
            note.detail.as_deref(),
            Some("1 of 2 recent commits were bug fixes")
        );
    }

    #[test]
    fn keys_are_stable_across_plans() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "a.rs", "fn a() {}\n", "t@e.x", "feat: add a");
        let cfg = Config::default();
        let ctx = TaskContext::new(dir.path(), &cfg).unwrap();
        let k1: Vec<String> = CommitIntent.plan(&ctx).unwrap()[0]
            .items
            .iter()
            .map(|i| i.key.clone())
            .collect();
        let k2: Vec<String> = CommitIntent.plan(&ctx).unwrap()[0]
            .items
            .iter()
            .map(|i| i.key.clone())
            .collect();
        assert_eq!(k1, k2);
    }
}
