//! Tasks that feed the drain order and the patch skills' decisions.
//!
//! Each adds an independent axis to a Finding as a `semantic` note. The
//! axes are compared one after another in `core::order` — never summed
//! into one score — and never change Tier or Severity, so Severity and
//! Hotspot stay orthogonal (`design-philosophy.md` §1.3).
//!
//! - `consequence` — what a failure in this file would cost, from
//!   development tooling to data integrity.
//! - `triage` — for drain-queue findings: the patch
//!   skill's gate (mechanical / false positive / escalate), the effort a
//!   fix takes, the accept reason if it is a false positive, and for
//!   duplication whether the copies express one idea.
//! - `friction` — for High / Critical complexity and LCOM: the
//!   three frictions `design-philosophy.md` §5.1 names (hard to change,
//!   hard to test, hard to read), asked separately, and the review
//!   skill's triage class.
//! - `focus` — with `--focus <file>`: how much the described work
//!   will touch each file. "Make the change easy, then make the easy
//!   change" (Kent Beck): refactor first what the next task will touch.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::core::config::DrainTier;
use crate::core::severity::Severity;
use crate::feature::Family;
use crate::semantic::api::{NoulCriteria, Question};
use crate::semantic::task::{Answered, Group, Item, Lowered, Task, TaskContext};
use crate::semantic::tasks::common::{criteria, finding_excerpt, note, numbered_range};

fn labels(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn score_label(levels: &[&str], answer: &crate::semantic::api::Answer) -> String {
    let (level, _) = crate::semantic::tasks::common::score_level(answer).unwrap_or((0.0, 0.0));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = (level.round().max(0.0) as usize).min(levels.len() - 1);
    levels[i].to_owned()
}

/// Files with at least one non-`Ok`, non-accepted Finding, sendable.
fn flagged_files(ctx: &TaskContext<'_>) -> BTreeSet<PathBuf> {
    ctx.findings
        .iter()
        .filter(|f| f.severity > Severity::Ok && !f.accepted)
        .map(|f| f.location.file.clone())
        .filter(|p| ctx.may_send(p))
        .collect()
}

/// Notes `name` on every non-`Ok` Finding of `file`.
fn note_file(
    ctx: &TaskContext<'_>,
    lowered: &mut Lowered,
    file: &Path,
    name: &str,
    n: &crate::core::finding::SemanticNote,
) {
    for f in ctx
        .findings
        .iter()
        .filter(|f| f.location.file == file && f.severity > Severity::Ok)
    {
        lowered
            .notes
            .push((f.id.clone(), name.to_owned(), n.clone()));
    }
}

// ---------------------------------------------------------------- consequence

pub struct Consequence;

pub const CONSEQUENCE_LABELS: [&str; 4] = ["dev", "internal", "user_facing", "critical"];
const CONSEQUENCE_LEVELS: [&str; 4] = [
    "Development only: tests, examples, scripts, build tooling, or generated code.",
    "Internal plumbing: helpers and infrastructure other code relies on, with no direct effect users see.",
    "User-facing behaviour: a defect here breaks something users or callers observe.",
    "Critical: authentication, authorization, payments, data integrity, security, or migrations, where a defect is costly or hard to undo.",
];
const CONSEQUENCE_INSTRUCTIONS: &str = "The state is the start of one source file. Rate what a defect in this file would cost the people who use the software.";

impl Task for Consequence {
    fn id(&self) -> &'static str {
        "consequence"
    }
    fn summary(&self) -> &'static str {
        "what a defect in each flagged file would cost (orders the drain queue)"
    }
    fn criteria_text(&self) -> String {
        format!(
            "{CONSEQUENCE_INSTRUCTIONS}\n{}",
            CONSEQUENCE_LEVELS.join("\n")
        )
    }
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let mut groups = Vec::new();
        for file in flagged_files(ctx) {
            let Some(src) = ctx.read_sendable(&file) else {
                continue;
            };
            let total = src.lines().count();
            let head = numbered_range(&src, 1, 200);
            let state = if total > 200 {
                format!(
                    "File: {} (first 200 of {total} lines)\n\n{head}",
                    file.display()
                )
            } else {
                format!("File: {}\n\n{head}", file.display())
            };
            groups.push(Group {
                items: vec![Item {
                    key: ctx.key(self, &file.to_string_lossy(), &state),
                    question: Question::Score {
                        instructions: json!(CONSEQUENCE_INSTRUCTIONS),
                        criteria: CONSEQUENCE_LEVELS.iter().map(|l| json!(l)).collect(),
                    },
                    meta: json!({"file": file.to_string_lossy()}),
                }],
                state: json!(state),
            });
        }
        Ok(groups)
    }
    fn lower(&self, ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        let mut lowered = Lowered::default();
        for a in answered {
            let Some(answer) = a.answer else { continue };
            let file = PathBuf::from(a.item.meta["file"].as_str().unwrap_or(""));
            let n = note(score_label(&CONSEQUENCE_LABELS, answer), answer, 4);
            note_file(ctx, &mut lowered, &file, "consequence", &n);
        }
        lowered
    }
}

// ---------------------------------------------------------- triage

pub struct Triage;

const GATE: [(&str, &str); 3] = [
    ("mechanical", "One listed mechanical refactoring (template method, lookup table, consolidating duplicate fragments, decomposing a condition, extracting a variable, naming a constant), or a direct fix of this finding, resolves it inside this file without a design decision."),
    ("false_positive", "The metric counts something intended: generated code, an exhaustive dispatch over a closed set, a parser or lookup table, vendored code, or a coherent pipeline that would lose meaning if split."),
    ("escalate", "Resolving it needs a design decision, a business rule, or coordinated changes across several files."),
];
const EFFORT: [(&str, &str); 3] = [
    ("local", "A change of a few lines inside one function."),
    (
        "contained",
        "Reworking one function or one class, within one file.",
    ),
    (
        "cross_file",
        "Changes that span several files or a module boundary.",
    ),
];
const REASONS_CODE: [(&str, &str); 7] = [
    ("generated_code", "Generated code."),
    (
        "exhaustive_enum_dispatch",
        "An exhaustive dispatch over a closed enum or set.",
    ),
    (
        "intentional_parser_table",
        "An intentional parser, lookup, or state table.",
    ),
    ("vendored_third_party", "Vendored third-party code."),
    (
        "coherent_pipeline_relocate_trap",
        "A coherent pipeline whose steps would only be relocated by splitting.",
    ),
    (
        "stateless_delegation",
        "A type with no fields (for example one trait implementation) whose methods delegate to free functions, so LCOM sees no shared state.",
    ),
    ("none", "None of these; the finding is real."),
];
const REASONS_TEST: [(&str, &str); 5] = [
    ("coverage_exercised_via_integration_suite", "The code is covered by an integration or end-to-end suite the unit coverage report does not see."),
    ("intentional_skip_environment_gated", "The tests skip on purpose outside a specific environment (hardware, CI, paid APIs)."),
    ("change_coupling_drift_test_lives_at_higher_layer", "The test lives at a higher layer and is not expected to change with this source."),
    ("coverage_pct_generated_or_vendored", "Generated, vendored, or schema-derived code that is not a test target."),
    ("none", "None of these; the finding is real."),
];
const REASONS_DOCS: [(&str, &str); 6] = [
    ("false_positive_observer_extracted_non_codebase_identifier", "The flagged identifier is a generic word, a third-party or standard-library name, not a codebase identifier."),
    ("pair_coverage_gap_identifier_exists_in_unpaired_src", "The identifier exists, in a source file the doc is not paired with."),
    ("false_positive_observer_slugify_diverges_from_github_slugger", "The anchor works in the site generator; the observer's slug rule differs."),
    ("false_positive_observer_counts_noun_phrase_TODO", "TODO / FIXME is used as a noun, not as an action item."),
    ("intentional_external_link", "The link target is intentionally outside the source tree."),
    ("none", "None of these; the finding is real."),
];
const TRIAGE_INSTRUCTIONS: &str = "The state is a finding HEAL reported and the code or text it is about. Answer about how this finding should be handled.";

impl Task for Triage {
    fn id(&self) -> &'static str {
        "triage"
    }
    fn summary(&self) -> &'static str {
        "drain-queue gate (mechanical / false positive / escalate), fix effort, and accept reason"
    }
    fn criteria_text(&self) -> String {
        let mut s = TRIAGE_INSTRUCTIONS.to_owned();
        for set in [
            &GATE[..],
            &EFFORT[..],
            &REASONS_CODE[..],
            &REASONS_TEST[..],
            &REASONS_DOCS[..],
        ] {
            for (k, v) in set {
                s.push_str(&format!("\n{k}: {v}"));
            }
        }
        s
    }
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let drain = &ctx.config.policy.drain;
        let mut groups = Vec::new();
        for f in ctx.findings {
            if f.accepted || !matches!(drain.tier_for(f), Some(DrainTier::Must | DrainTier::Should))
            {
                continue;
            }
            let Some(excerpt) = finding_excerpt(ctx, f) else {
                continue;
            };
            let state = format!(
                "Finding `{}` ({}, {:?}) at {}:\n{}\n\n{excerpt}",
                f.metric,
                f.summary,
                f.severity,
                f.location.file.display(),
                f.fix_hint.as_deref().unwrap_or("")
            );
            let reasons: &[(&str, &str)] = match Family::for_metric(&f.metric) {
                Family::Code => &REASONS_CODE,
                Family::Test => &REASONS_TEST,
                Family::Docs => &REASONS_DOCS,
            };
            let mut items = vec![
                Item {
                    key: ctx.key(self, "gate", &state),
                    question: Question::Choice {
                        instructions: json!(format!("{TRIAGE_INSTRUCTIONS} Which handling fits?")),
                        criteria: criteria(&labels(&GATE)),
                    },
                    meta: json!({"id": f.id, "note": "gate"}),
                },
                Item {
                    key: ctx.key(self, "effort", &state),
                    question: Question::Choice {
                        instructions: json!(format!("{TRIAGE_INSTRUCTIONS} How large is the change that resolves it?")),
                        criteria: criteria(&labels(&EFFORT)),
                    },
                    meta: json!({"id": f.id, "note": "effort"}),
                },
                Item {
                    key: ctx.key(self, "accept_reason", &state),
                    question: Question::Choice {
                        instructions: json!(format!("{TRIAGE_INSTRUCTIONS} If this is a false positive, which reason applies?")),
                        criteria: criteria(&labels(reasons)),
                    },
                    meta: json!({"id": f.id, "note": "accept_reason"}),
                },
            ];
            if f.metric == "duplication" {
                items.push(Item {
                    key: ctx.key(self, "duplication_real", &state),
                    question: Question::Noul {
                        instructions: json!("The duplicated sites express one idea that should live in one place, rather than a coincidental similarity between different ideas."),
                        criteria: Some(NoulCriteria {
                            yes: json!("One idea, copied."),
                            no: json!("Coincidental similarity; the sites would change for different reasons."),
                        }),
                    },
                    meta: json!({"id": f.id, "note": "duplication_real"}),
                });
            }
            groups.push(Group {
                state: json!(state),
                items,
            });
        }
        Ok(groups)
    }
    fn lower(&self, _ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        let mut lowered = Lowered::default();
        for a in answered {
            let Some(answer) = a.answer else { continue };
            let name = a.item.meta["note"].as_str().unwrap_or("").to_owned();
            let label = crate::semantic::tasks::common::chosen(answer).map_or_else(
                || {
                    if answer.strength(0) >= 0.5 {
                        "true".to_owned()
                    } else {
                        "false".to_owned()
                    }
                },
                |(l, _, _)| l.to_owned(),
            );
            let id = a.item.meta["id"].as_str().unwrap_or("").to_owned();
            lowered.notes.push((id, name, note(label, answer, 0)));
        }
        lowered
    }
}

// ----------------------------------------------------------- friction

pub struct Friction;

const FRICTIONS: [(&str, &str); 3] = [
    ("change", "Changing this code safely requires reading and understanding a lot of other code around it."),
    ("test", "Writing a test that checks this code's behaviour is hard: it needs heavy setup, hidden dependencies, or cannot be isolated."),
    ("read", "A reader new to the codebase cannot follow what this code does or why."),
];
const TRIAGE_CLASS: [(&str, &str); 3] = [
    ("symptomatic", "Duplicated logic, a class with mixed responsibilities, or coupling between layers: fixing it removes real friction."),
    ("intrinsic", "Intrinsically complex: a graph traversal, a statistical aggregation, an exhaustive match over a closed set, data-shaped conditions. Refactoring would relocate or destroy meaning."),
    ("cohesive_procedural", "A cohesive procedure with sequential phases (an event handler, an emit pipeline, an orchestrator of coherent steps). Splitting it would only relocate the score."),
];
const FRICTION_INSTRUCTIONS: &str = "The state is code HEAL flagged as complex or incohesive. Answer about the friction it causes a developer.";

impl Task for Friction {
    fn id(&self) -> &'static str {
        "friction"
    }
    fn summary(&self) -> &'static str {
        "hard to change / test / read, and symptomatic vs intrinsic, for High+ complexity and LCOM"
    }
    fn criteria_text(&self) -> String {
        let mut s = FRICTION_INSTRUCTIONS.to_owned();
        for (k, v) in FRICTIONS.iter().chain(TRIAGE_CLASS.iter()) {
            s.push_str(&format!("\n{k}: {v}"));
        }
        s
    }
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let mut groups = Vec::new();
        for f in ctx.findings {
            if f.accepted
                || f.severity < Severity::High
                || !matches!(f.metric.as_str(), "ccn" | "cognitive" | "lcom")
            {
                continue;
            }
            let Some(excerpt) = finding_excerpt(ctx, f) else {
                continue;
            };
            let state = format!(
                "{} — {} ({})\n\n{excerpt}",
                f.metric,
                f.summary,
                f.location.file.display()
            );
            let mut items: Vec<Item> = FRICTIONS
                .iter()
                .map(|(k, q)| Item {
                    key: ctx.key(self, k, &state),
                    question: Question::Noul {
                        instructions: json!(q),
                        criteria: None,
                    },
                    meta: json!({"id": f.id, "note": format!("friction.{k}")}),
                })
                .collect();
            items.push(Item {
                key: ctx.key(self, "triage_class", &state),
                question: Question::Choice {
                    instructions: json!(format!(
                        "{FRICTION_INSTRUCTIONS} Which kind of complexity is it?"
                    )),
                    criteria: criteria(&labels(&TRIAGE_CLASS)),
                },
                meta: json!({"id": f.id, "note": "triage_class"}),
            });
            groups.push(Group {
                state: json!(state),
                items,
            });
        }
        Ok(groups)
    }
    fn lower(&self, _ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        let mut lowered = Lowered::default();
        for a in answered {
            let Some(answer) = a.answer else { continue };
            let name = a.item.meta["note"].as_str().unwrap_or("").to_owned();
            let label = crate::semantic::tasks::common::chosen(answer).map_or_else(
                || name.trim_start_matches("friction.").to_owned(),
                |(l, _, _)| l.to_owned(),
            );
            let id = a.item.meta["id"].as_str().unwrap_or("").to_owned();
            lowered.notes.push((id, name, note(label, answer, 0)));
        }
        lowered
    }
}

// ---------------------------------------------------------------- focus

pub struct Focus;

pub const FOCUS_LABELS: [&str; 4] = ["none", "read", "touch", "change"];
const FOCUS_LEVELS: [&str; 4] = [
    "The described work does not involve this file.",
    "The work may need to read this file to understand the code, but not change it.",
    "The work will likely change this file a little.",
    "The work will change this file substantially.",
];
const FOCUS_INSTRUCTIONS: &str = "The state describes upcoming work on a codebase. Rate how much that work will involve the file shown in the question.";
/// Files asked about per focus.
const MAX_FOCUS_FILES: usize = 300;
/// Characters of the focus text sent.
const MAX_FOCUS_CHARS: usize = 20_000;

impl Task for Focus {
    fn id(&self) -> &'static str {
        "focus"
    }
    fn summary(&self) -> &'static str {
        "with --focus <file>: how much the described work touches each flagged file"
    }
    /// One person's planned work is not team state.
    fn shared(&self) -> bool {
        false
    }
    fn criteria_text(&self) -> String {
        format!("{FOCUS_INSTRUCTIONS}\n{}", FOCUS_LEVELS.join("\n"))
    }
    fn setup_hint(&self, ctx: &TaskContext<'_>) -> Option<String> {
        ctx.focus
            .is_none()
            .then(|| "pass --focus <file> (a plan, an issue, or a task description) to rank for upcoming work".to_owned())
    }
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let Some(focus) = ctx.focus else {
            return Ok(Vec::new());
        };
        let focus: String = focus.chars().take(MAX_FOCUS_CHARS).collect();
        let state = format!("Upcoming work:\n{focus}");
        let items: Vec<Item> = flagged_files(ctx)
            .into_iter()
            .take(MAX_FOCUS_FILES)
            .filter_map(|file| {
                let src = ctx.read_sendable(&file)?;
                let head = numbered_range(&src, 1, 40);
                let subject = format!("File: {}\n{head}", file.display());
                Some(Item {
                    key: ctx.key(self, &subject, &state),
                    question: Question::Score {
                        instructions: json!(format!("{FOCUS_INSTRUCTIONS}\n\n{subject}")),
                        criteria: FOCUS_LEVELS.iter().map(|l| json!(l)).collect(),
                    },
                    meta: json!({"file": file.to_string_lossy()}),
                })
            })
            .collect();
        if items.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![Group {
            state: json!(state),
            items,
        }])
    }
    fn lower(&self, ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        let mut lowered = Lowered::default();
        for a in answered {
            let Some(answer) = a.answer else { continue };
            let file = PathBuf::from(a.item.meta["file"].as_str().unwrap_or(""));
            let n = note(score_label(&FOCUS_LABELS, answer), answer, 4);
            note_file(ctx, &mut lowered, &file, "focus", &n);
        }
        lowered
    }
}
