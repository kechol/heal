//! On-demand verification tasks: an independent check of what an agent
//! just wrote, instead of the agent grading its own work.
//!
//! - `verify_patch` — given `--diff <range>`, does the change
//!   only relocate complexity, reflexively flip a flat condition into
//!   negated guard clauses, change behaviour, or carry a commit message
//!   that does not describe it? The patch skills run this after each
//!   commit and revert a commit that fails.
//! - `verify_proposal` — given `--focus proposals.json`, the five
//!   readability questions of `plugins/heal/skills/refactor/references/readability.md`
//!   §3, one question each.
//!
//! Both report through `heal semantic ask --json` (`tasks[].result`).
//! Neither produces Findings.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use git2::{DiffFormat, Repository};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::semantic::api::{NoulCriteria, Question};
use crate::semantic::task::{Answered, Group, Item, Task, TaskContext};
use crate::semantic::tasks::common::{noul_p, numbered};

/// Flags at or above this probability fail the check.
const DEFAULT_FLAG_CUTOFF: f64 = 0.7;

/// The patch of `range` (`A..B`, or a single revision meaning `rev^..rev`)
/// per file, plus the commit messages in the range. Files excluded from
/// sending are left out.
pub(crate) fn range_diff(
    ctx: &TaskContext<'_>,
    range: &str,
) -> anyhow::Result<(Vec<(PathBuf, String)>, String)> {
    let repo = Repository::discover(ctx.project)?;
    let (from, to) = if let Some((a, b)) = range.split_once("..") {
        let to = if b.is_empty() { "HEAD" } else { b };
        (
            repo.revparse_single(a)?.peel_to_commit()?,
            repo.revparse_single(to)?.peel_to_commit()?,
        )
    } else {
        let to = repo.revparse_single(range)?.peel_to_commit()?;
        (to.parent(0)?, to)
    };
    let diff = repo.diff_tree_to_tree(Some(&from.tree()?), Some(&to.tree()?), None)?;
    let mut per_file: BTreeMap<PathBuf, String> = BTreeMap::new();
    diff.print(DiffFormat::Patch, |delta, _hunk, line| {
        let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) else {
            return true;
        };
        if !ctx.may_send(path) {
            return true;
        }
        let text = per_file.entry(path.to_path_buf()).or_default();
        let origin = line.origin();
        if matches!(origin, '+' | '-' | ' ') {
            text.push(origin);
        }
        text.push_str(&String::from_utf8_lossy(line.content()));
        true
    })?;
    let mut walk = repo.revwalk()?;
    walk.push(to.id())?;
    walk.hide(from.id())?;
    let mut messages = String::new();
    for oid in walk.filter_map(Result::ok) {
        if let Ok(c) = repo.find_commit(oid) {
            let _ = writeln!(
                messages,
                "commit {}\n{}\n",
                &oid.to_string()[..8],
                c.message().unwrap_or("").trim()
            );
        }
    }
    Ok((per_file.into_iter().collect(), messages))
}

fn noul(instructions: &str, yes: &str, no: &str) -> Question {
    Question::Noul {
        instructions: json!(instructions),
        criteria: Some(NoulCriteria {
            yes: json!(yes),
            no: json!(no),
        }),
    }
}

pub struct VerifyPatch;

const PATCH_CHECKS: [(&str, &str, &str, &str); 4] = [
    (
        "relocate",
        "Judge whether this change moves complexity into new or other functions or files instead of removing it, so that a reader still has to follow about as many branches as before.",
        "The complexity was moved, not removed.",
        "The change removes or genuinely simplifies the logic.",
    ),
    (
        "guard_clause",
        "Judge whether this change turns a flat, non-nested condition into a chain of negated early returns without reducing nesting.",
        "A flat condition became a chain of negated early returns.",
        "No such rewrite, or the rewrite removes real nesting.",
    ),
    (
        "behaviour_change",
        "Judge whether this change alters what the code does for some input, beyond how the code is structured.",
        "Observable behaviour changes for some input.",
        "Only the structure changes; behaviour is the same for every input.",
    ),
    (
        "message_mismatch",
        "Judge whether the commit message claims something this change does not do, or leaves out the main thing it does.",
        "The message does not describe the change.",
        "The message describes the change.",
    ),
];

impl Task for VerifyPatch {
    fn id(&self) -> &'static str {
        "verify_patch"
    }

    fn summary(&self) -> &'static str {
        "on demand: check a commit for relocated complexity, reflexive guard clauses, behaviour change, and a mismatched message (`--diff`)"
    }

    fn on_demand(&self) -> bool {
        true
    }

    fn criteria_text(&self) -> String {
        PATCH_CHECKS
            .iter()
            .map(|c| format!("{}: {} {} / {}\n", c.0, c.1, c.2, c.3))
            .collect()
    }

    fn setup_hint(&self, _ctx: &TaskContext<'_>) -> Option<String> {
        Some("pass --diff <range>, e.g. --diff HEAD~1..HEAD".to_owned())
    }

    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let Some(range) = ctx.diff_range else {
            return Ok(Vec::new());
        };
        let (files, messages) = range_diff(ctx, range)?;
        if files.is_empty() {
            return Ok(Vec::new());
        }
        let whole: String = files
            .iter()
            .map(|(p, t)| format!("--- {}\n{t}\n", p.display()))
            .collect();
        let budget = crate::semantic::cost::state_budget();
        // One state for the whole change when it fits; otherwise one per
        // file (the message check then runs against each file's slice).
        let states: Vec<String> = if crate::semantic::cost::estimate_tokens(
            whole.len() + messages.len(),
        ) <= budget
        {
            vec![format!("Commit messages:\n{messages}\nDiff:\n{whole}")]
        } else {
            files
                .iter()
                .map(|(p, t)| format!("Commit messages:\n{messages}\nDiff (one file of a larger change):\n--- {}\n{t}", p.display()))
                .collect()
        };
        Ok(states
            .into_iter()
            .map(|state| Group {
                items: PATCH_CHECKS
                    .iter()
                    .map(|(name, q, yes, no)| Item {
                        key: ctx.key(self, name, &state),
                        question: noul(q, yes, no),
                        meta: json!({"check": name}),
                    })
                    .collect(),
                state: json!(state),
            })
            .collect())
    }

    fn report(&self, ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Option<Value> {
        if answered.is_empty() {
            return None;
        }
        let cutoff = ctx.cutoff(self, DEFAULT_FLAG_CUTOFF);
        let mut worst: BTreeMap<String, f64> = BTreeMap::new();
        let mut unanswered = 0;
        for a in answered {
            let check = a.item.meta["check"].as_str().unwrap_or("").to_owned();
            match a.answer.and_then(noul_p) {
                Some(p) => {
                    let e = worst.entry(check).or_insert(0.0);
                    *e = e.max(p);
                }
                None => unanswered += 1,
            }
        }
        // A behaviour change is only a failure for a refactor; the skill
        // decides, so it is reported but not in `flags`.
        let flags: Vec<&String> = worst
            .iter()
            .filter(|(k, p)| k.as_str() != "behaviour_change" && **p >= cutoff)
            .map(|(k, _)| k)
            .collect();
        Some(json!({
            "range": ctx.diff_range,
            "checks": worst,
            "flags": flags,
            "pass": flags.is_empty() && unanswered == 0,
            "unanswered": unanswered,
            "cutoff": cutoff,
        }))
    }
}

pub struct VerifyProposal;

/// `plugins/heal/skills/refactor/references/readability.md` §3, phrased so that
/// "true" is the good outcome.
const PROPOSAL_CHECKS: [(&str, &str); 5] = [
    ("reads_faster", "After the proposed change, a reader would understand this code faster than before."),
    ("hides_complexity", "The proposed change hides complexity behind a well-named abstraction instead of only moving it somewhere else."),
    ("preserves_rules", "The proposed change keeps business rules readable in the form the specification states them."),
    ("real_seam", "The proposed change separates parts that each have an independent meaning, rather than cutting one coherent procedure apart."),
    ("stranger_prefers", "A reader new to this codebase would prefer the code after the proposed change."),
];

#[derive(Debug, Deserialize)]
struct Proposals {
    proposals: Vec<Proposal>,
}

#[derive(Debug, Deserialize)]
struct Proposal {
    id: String,
    text: String,
    #[serde(default)]
    files: Vec<PathBuf>,
}

impl Task for VerifyProposal {
    fn id(&self) -> &'static str {
        "verify_proposal"
    }

    fn summary(&self) -> &'static str {
        "on demand: the five readability questions for each review proposal (`--focus proposals.json`)"
    }

    fn on_demand(&self) -> bool {
        true
    }

    fn criteria_text(&self) -> String {
        PROPOSAL_CHECKS
            .iter()
            .map(|(k, q)| format!("{k}: {q}\n"))
            .collect()
    }

    fn setup_hint(&self, _ctx: &TaskContext<'_>) -> Option<String> {
        Some(
            r#"pass --focus with {"proposals":[{"id":"1","text":"…","files":["src/a.rs"]}]}"#
                .to_owned(),
        )
    }

    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let Some(raw) = ctx.focus else {
            return Ok(Vec::new());
        };
        let parsed: Proposals = serde_json::from_str(raw).map_err(|e| {
            anyhow::anyhow!("--focus for verify_proposal must be proposals JSON: {e}")
        })?;
        let budget = crate::semantic::cost::state_budget();
        let mut groups = Vec::new();
        for p in parsed.proposals {
            let mut state = format!("Proposed change:\n{}\n", p.text);
            for f in &p.files {
                if let Some(src) = ctx.read_sendable(f) {
                    let block = format!("\nCurrent code, {}:\n{}", f.display(), numbered(&src));
                    if crate::semantic::cost::estimate_tokens(state.len() + block.len()) <= budget {
                        state.push_str(&block);
                    }
                }
            }
            let items = PROPOSAL_CHECKS
                .iter()
                .map(|(name, q)| Item {
                    key: ctx.key(self, name, &state),
                    question: noul(q, "Yes.", "No, or it is unclear."),
                    meta: json!({"proposal": p.id, "check": name}),
                })
                .collect();
            groups.push(Group {
                state: json!(state),
                items,
            });
        }
        Ok(groups)
    }

    fn report(&self, ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Option<Value> {
        let cutoff = ctx.cutoff(self, 0.5);
        let mut by: BTreeMap<String, BTreeMap<String, Option<f64>>> = BTreeMap::new();
        for a in answered {
            let id = a.item.meta["proposal"].as_str().unwrap_or("").to_owned();
            let check = a.item.meta["check"].as_str().unwrap_or("").to_owned();
            by.entry(id)
                .or_default()
                .insert(check, a.answer.and_then(noul_p));
        }
        let rows: Vec<Value> = by
            .into_iter()
            .map(|(id, checks)| {
                // Mirrors readability.md: any "no" or "unsure" defers.
                let pass = checks.values().all(|p| p.is_some_and(|p| p >= cutoff));
                json!({"id": id, "checks": checks, "pass": pass})
            })
            .collect();
        Some(json!({"proposals": rows, "cutoff": cutoff}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;
    use crate::semantic::tasks::testing::noul as answer_noul;
    use crate::test_support::{commit, init_repo};

    #[test]
    fn verify_patch_plans_four_checks_and_reports_flags() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        commit(dir.path(), "a.rs", "fn a() {}\n", "t@e.x", "init");
        commit(
            dir.path(),
            "a.rs",
            "fn a() { b(); }\nfn b() {}\n",
            "t@e.x",
            "refactor: extract b",
        );
        let cfg = Config::default();
        let mut ctx = TaskContext::new(dir.path(), &cfg).unwrap();
        ctx.diff_range = Some("HEAD~1..HEAD");
        let groups = VerifyPatch.plan(&ctx).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].items.len(), 4);
        let state = groups[0].state.as_str().unwrap();
        assert!(
            state.contains("refactor: extract b") && state.contains("+fn b() {}"),
            "{state}"
        );

        let answers: Vec<_> = groups[0]
            .items
            .iter()
            .map(|i| {
                answer_noul(if i.meta["check"] == "relocate" {
                    0.9
                } else {
                    0.1
                })
            })
            .collect();
        let answered: Vec<Answered<'_>> = groups[0]
            .items
            .iter()
            .zip(&answers)
            .map(|(item, a)| Answered {
                item,
                answer: Some(a),
            })
            .collect();
        let r = VerifyPatch.report(&ctx, &answered).unwrap();
        assert_eq!(r["flags"], json!(["relocate"]));
        assert_eq!(r["pass"], json!(false));
    }

    #[test]
    fn verify_proposal_defers_on_any_no() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::default();
        let mut ctx = TaskContext::new(dir.path(), &cfg).unwrap();
        let focus = r#"{"proposals":[{"id":"p1","text":"extract a helper"}]}"#;
        ctx.focus = Some(focus);
        let groups = VerifyProposal.plan(&ctx).unwrap();
        assert_eq!(groups[0].items.len(), 5);
        let answers: Vec<_> = groups[0]
            .items
            .iter()
            .enumerate()
            .map(|(i, _)| answer_noul(if i == 2 { 0.2 } else { 0.9 }))
            .collect();
        let answered: Vec<Answered<'_>> = groups[0]
            .items
            .iter()
            .zip(&answers)
            .map(|(item, a)| Answered {
                item,
                answer: Some(a),
            })
            .collect();
        let r = VerifyProposal.report(&ctx, &answered).unwrap();
        assert_eq!(r["proposals"][0]["pass"], json!(false));
    }
}
