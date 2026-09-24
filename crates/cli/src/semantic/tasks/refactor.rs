//! Tasks that help drain CCN / Cognitive / duplication findings.
//!
//! - `split_points` — for a High / Critical CCN or Cognitive
//!   function, ask for each pair of adjacent statement blocks whether
//!   they serve one step of the function. Low answers are the boundaries
//!   where an extracted function would have one purpose of its own; the
//!   agent then names the pieces. This is a semantic cut, not "move this
//!   branch out", which is how HEAL avoids the relocate trap
//!   (`design-philosophy.md` §5.2).
//! - `fix_pattern` — which of the patch skill's allow-listed
//!   refactorings fits a drain-queue finding, or none. Replaces the
//!   agent's own pattern pick with a scored one.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::json;
use tree_sitter::Node;

use crate::core::config::DrainTier;
use crate::core::finding::SemanticNote;
use crate::core::severity::Severity;
use crate::feature::Family;
use crate::observer::code::complexity::outer_functions;
use crate::semantic::api::{NoulCriteria, Question};
use crate::semantic::task::{Answered, Group, Item, Lowered, Task, TaskContext};
use crate::semantic::tasks::common::{
    chosen, criteria, file_facts, labels, note, noul_p, numbered_range, parse_file,
};

/// A segment must span at least this many lines before a boundary is asked.
const MIN_SEGMENT_LINES: u32 = 3;
/// Boundaries asked per function.
const MAX_SEGMENTS: usize = 30;

pub struct SplitPoints;

const SPLIT_INSTRUCTIONS: &str = "The state is one function with line numbers. Decide whether the two consecutive blocks of statements named below work toward the same single step of what the function does, so that cutting between them would split one step in two.";

fn statement_spans(body: Node<'_>) -> Vec<(u32, u32)> {
    let mut cur = body.walk();
    body.named_children(&mut cur)
        .filter(|n| !n.kind().contains("comment"))
        .map(|n| {
            (
                u32::try_from(n.start_position().row + 1).unwrap_or(u32::MAX),
                u32::try_from(n.end_position().row + 1).unwrap_or(u32::MAX),
            )
        })
        .collect()
}

/// Merge statements into segments of at least `MIN_SEGMENT_LINES` lines,
/// then cap the count by merging the shortest neighbours.
fn segments(spans: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for &(s, e) in spans {
        match out.last_mut() {
            Some(last) if last.1 - last.0 + 1 < MIN_SEGMENT_LINES => last.1 = e,
            _ => out.push((s, e)),
        }
    }
    while out.len() > MAX_SEGMENTS {
        let i = (0..out.len() - 1)
            .min_by_key(|&i| out[i + 1].1 - out[i].0)
            .unwrap_or(0);
        out[i].1 = out[i + 1].1;
        out.remove(i + 1);
    }
    out
}

impl Task for SplitPoints {
    fn id(&self) -> &'static str {
        "split_points"
    }

    fn summary(&self) -> &'static str {
        "where a High / Critical CCN or Cognitive function splits into steps with one purpose each"
    }

    fn criteria_text(&self) -> String {
        SPLIT_INSTRUCTIONS.to_owned()
    }

    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        // (file, symbol) → finding ids to decorate
        let mut targets: BTreeMap<(PathBuf, String), Vec<String>> = BTreeMap::new();
        for f in ctx.findings {
            if matches!(f.metric.as_str(), "ccn" | "cognitive") && f.severity >= Severity::High {
                if let Some(s) = &f.location.symbol {
                    targets
                        .entry((f.location.file.clone(), s.clone()))
                        .or_default()
                        .push(f.id.clone());
                }
            }
        }
        let mut groups = Vec::new();
        for ((file, symbol), ids) in targets {
            let Some(parsed) = parse_file(ctx, &file) else {
                continue;
            };
            let Some(func) = outer_functions(&parsed)
                .into_iter()
                .find(|f| f.name == symbol)
            else {
                continue;
            };
            let Some(node) = parsed
                .tree
                .root_node()
                .descendant_for_byte_range(func.byte_range.start, func.byte_range.end)
            else {
                continue;
            };
            let body = node.child_by_field_name("body").unwrap_or(node);
            let segs = segments(&statement_spans(body));
            if segs.len() < 3 {
                continue;
            }
            let state = format!(
                "File: {} — function `{symbol}`\n\n{}",
                file.display(),
                numbered_range(&parsed.source, func.start_row, func.end_row)
            );
            let items = segs
                .windows(2)
                .map(|w| {
                    let subject = format!("lines {}–{} and lines {}–{}", w[0].0, w[0].1, w[1].0, w[1].1);
                    Item {
                        key: ctx.key(self, &subject, &state),
                        question: Question::Noul {
                            instructions: json!(format!("{SPLIT_INSTRUCTIONS}\nBlocks: {subject}")),
                            criteria: Some(NoulCriteria {
                                yes: json!("Both blocks are part of the same step."),
                                no: json!("The second block starts a new step with its own purpose."),
                            }),
                        },
                        meta: json!({"ids": ids, "boundary": w[1].0, "first": segs[0].0, "last": segs[segs.len() - 1].1}),
                    }
                })
                .collect();
            groups.push(Group {
                state: json!(state),
                items,
            });
        }
        Ok(groups)
    }

    fn lower(&self, ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        let cutoff = ctx.cutoff(self, 0.7);
        // finding id → (split lines, min p, first, last)
        let mut per: BTreeMap<String, (Vec<u32>, f64, u32, u32)> = BTreeMap::new();
        for a in answered {
            let Some(p) = a.answer.and_then(noul_p) else {
                continue;
            };
            let m = &a.item.meta;
            let ids: Vec<String> = m["ids"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .filter_map(|s| s.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let boundary = u32::try_from(m["boundary"].as_u64().unwrap_or(0)).unwrap_or(0);
            let (first, last) = (
                u32::try_from(m["first"].as_u64().unwrap_or(0)).unwrap_or(0),
                u32::try_from(m["last"].as_u64().unwrap_or(0)).unwrap_or(0),
            );
            for id in ids {
                let e = per.entry(id).or_insert((Vec::new(), 1.0, first, last));
                if p <= 1.0 - cutoff {
                    e.0.push(boundary);
                    e.1 = e.1.min(p);
                }
            }
        }
        let mut lowered = Lowered::default();
        for (id, (mut lines, min_p, first, last)) in per {
            if lines.is_empty() {
                continue;
            }
            lines.sort_unstable();
            let mut steps = Vec::new();
            let mut start = first;
            for &b in &lines {
                steps.push(format!("{start}–{}", b.saturating_sub(1)));
                start = b;
            }
            steps.push(format!("{start}–{last}"));
            lowered.notes.push((
                id,
                "split_points".to_owned(),
                SemanticNote {
                    label: "split".to_owned(),
                    p: 1.0 - min_p,
                    confidence: (1.0 - min_p - 0.5).abs() * 2.0,
                    lines,
                    detail: Some(format!("candidate steps: lines {}", steps.join(", "))),
                },
            ));
        }
        lowered
    }
}

pub struct FixPattern;

/// The patch skill's allow-list (`heal-code-patch` SKILL.md), plus `none`.
pub const PATTERNS: [(&str, &str); 7] = [
    ("form_template_method", "Form Template Method: several call sites are identical except for one varying part (a predicate, transform, or message), which becomes a parameter."),
    ("lookup_table", "Replace Conditional with Lookup Table: the conditional is a pure equality cascade with no side effects or early returns."),
    ("consolidate_fragments", "Consolidate Duplicate Conditional Fragments: every branch ends with the same statements, which move after the conditional."),
    ("decompose_conditional", "Decompose Conditional: a condition or branch body becomes a named function whose signature is much narrower than its body."),
    ("extract_variable", "Extract Variable: an intermediate computation used two or more times gets a name that reveals intent."),
    ("named_constant", "Replace Magic Number or String with Named Constant: a fixed value with one meaning appears in several places."),
    ("none", "None of these fits; the complexity is intrinsic or needs a design change, not a mechanical rewrite."),
];

const PATTERN_INSTRUCTIONS: &str = "The state is code HEAL flagged as too complex or duplicated. Choose the one mechanical refactoring that would remove the problem without changing behaviour, or none if no listed refactoring fits.";

impl Task for FixPattern {
    fn id(&self) -> &'static str {
        "fix_pattern"
    }

    fn summary(&self) -> &'static str {
        "which allow-listed refactoring fits each drain-queue CCN / Cognitive / duplication finding"
    }

    fn criteria_text(&self) -> String {
        let mut s = PATTERN_INSTRUCTIONS.to_owned();
        for (k, v) in PATTERNS {
            s.push('\n');
            s.push_str(k);
            s.push_str(": ");
            s.push_str(v);
        }
        s
    }

    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let labels = labels(&PATTERNS);
        let drain = &ctx.config.policy.drain;
        let mut groups = Vec::new();
        for f in ctx.findings {
            if !matches!(f.metric.as_str(), "ccn" | "cognitive" | "duplication")
                || Family::for_metric(&f.metric) != Family::Code
                || !matches!(drain.tier_for(f), Some(DrainTier::Must | DrainTier::Should))
            {
                continue;
            }
            let Some(facts) = file_facts(ctx, &f.location.file) else {
                continue;
            };
            let excerpt = if f.metric == "duplication" {
                let mut sites = vec![&f.location];
                sites.extend(f.locations.iter().take(2));
                let mut s = String::new();
                for loc in sites {
                    let line = loc.line.unwrap_or(1);
                    let text = ctx
                        .read_sendable(&loc.file)
                        .map(|src| numbered_range(&src, line, line + 30))
                        .unwrap_or_default();
                    s.push_str(&format!("Site {}:{line}\n{text}\n", loc.file.display()));
                }
                s
            } else {
                let symbol = f.location.symbol.as_deref().unwrap_or("");
                let Some(func) = facts.functions.iter().find(|x| x.name == symbol) else {
                    continue;
                };
                format!(
                    "Function `{symbol}` in {}\n{}",
                    f.location.file.display(),
                    numbered_range(&facts.source, func.start_row, func.end_row)
                )
            };
            let state = format!("Finding: {} — {}\n\n{excerpt}", f.metric, f.summary);
            groups.push(Group {
                state: json!(state.clone()),
                items: vec![Item {
                    key: ctx.key(self, &f.id, &state),
                    question: Question::Choice {
                        instructions: json!(PATTERN_INSTRUCTIONS),
                        criteria: criteria(&labels),
                    },
                    meta: json!({"id": f.id}),
                }],
            });
        }
        Ok(groups)
    }

    fn lower(&self, _ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        let mut lowered = Lowered::default();
        for a in answered {
            let Some(answer) = a.answer else {
                continue;
            };
            let Some((label, _, _)) = chosen(answer) else {
                continue;
            };
            let id = a.item.meta["id"].as_str().unwrap_or_default().to_owned();
            lowered
                .notes
                .push((id, "fix_pattern".to_owned(), note(label, answer, 0)));
        }
        lowered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_merge_short_statements_and_cap() {
        let spans = [(1, 1), (2, 2), (3, 3), (4, 10), (11, 11), (12, 20)];
        assert_eq!(segments(&spans), [(1, 3), (4, 10), (11, 20)]);
        let many: Vec<(u32, u32)> = (0..100).map(|i| (i * 4 + 1, i * 4 + 4)).collect();
        assert_eq!(segments(&many).len(), MAX_SEGMENTS);
    }

    #[test]
    fn split_notes_collect_low_boundaries_into_steps() {
        let items: Vec<Item> = [(11, 0.9), (21, 0.1), (31, 0.2)]
            .iter()
            .map(|(b, _)| Item {
                key: format!("k{b}"),
                question: Question::Noul {
                    instructions: json!(""),
                    criteria: None,
                },
                meta: json!({"ids": ["f1"], "boundary": b, "first": 1, "last": 40}),
            })
            .collect();
        let answers: Vec<_> = [0.9, 0.1, 0.2]
            .iter()
            .map(|p| crate::semantic::tasks::testing::noul(*p))
            .collect();
        let answered: Vec<Answered<'_>> = items
            .iter()
            .zip(&answers)
            .map(|(item, a)| Answered {
                item,
                answer: Some(a),
            })
            .collect();
        let cfg = crate::core::config::Config::default();
        let ctx = TaskContext::new(std::path::Path::new("."), &cfg).unwrap();
        let lowered = SplitPoints.lower(&ctx, &answered);
        assert_eq!(lowered.notes.len(), 1);
        let n = &lowered.notes[0].2;
        assert_eq!(n.lines, [21, 31]);
        assert_eq!(
            n.detail.as_deref(),
            Some("candidate steps: lines 1–20, 21–30, 31–40")
        );
    }
}
