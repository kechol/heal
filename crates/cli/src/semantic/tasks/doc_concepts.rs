//! Reading the docs through the concept vocabulary.
//!
//! - `doc_concept` — classify every doc section into a concept of
//!   `.heal/concepts.toml` (the same vocabulary the `concept` task maps the
//!   code with). A concept that holds a real share of the code but no doc
//!   section → `doc_concept.gap`: coverage by meaning, not by file pair.
//! - `doc_overlap` — for sections of different pages that explain the
//!   same concept: do they say the same thing (`doc_concept.duplicate`,
//!   keep one and link to it) or contradict each other
//!   (`doc_concept.conflict`)? Catches reworded duplication that the
//!   token-exact Markdown duplication pass cannot.
//! - `doc_pairs` (on demand) — for docs without a pair, which source
//!   file the page mainly documents, with a real probability. Replaces the
//!   fixed confidence the pair-setup skill used for its own guesses.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::core::concepts::OTHER;
use crate::core::severity::Severity;
use crate::observer::docs::sections::{sections, Section};
use crate::semantic::api::{NoulCriteria, Question};
use crate::semantic::task::{Answered, Group, Item, Lowered, Task, TaskContext};
use crate::semantic::tasks::common::{
    chosen, code_files, criteria, file_states, finding, noul_p, numbered_range,
};
use crate::semantic::tasks::concept::{load_concepts, ConceptTask};
use crate::semantic::tasks::doc_family::doc_files;

/// A concept "needs" a doc when it holds this share of classified code…
const GAP_SHARE: f64 = 0.1;
/// …or this many lines.
const GAP_MIN_LOC: u32 = 300;
/// Sections shorter than this are not compared for overlap.
const MIN_OVERLAP_LINES: u32 = 5;
const MAX_OVERLAP_PAIRS_PER_CONCEPT: usize = 10;

fn doc_sections(body: &str) -> Vec<Section> {
    sections(body)
        .into_iter()
        .filter(|s| s.level > 0 && !s.is_empty())
        .collect()
}

pub struct DocConcept;

const DOC_CONCEPT_INSTRUCTIONS: &str = "The state is a documentation page with line numbers. Decide which concept the named section mainly explains.";

impl Task for DocConcept {
    fn id(&self) -> &'static str {
        "doc_concept"
    }
    fn summary(&self) -> &'static str {
        "map doc sections to concepts; flag concepts the code relies on that no doc explains"
    }
    fn depends_on(&self) -> &'static [&'static str] {
        &["concept"]
    }
    fn criteria_text(&self) -> String {
        DOC_CONCEPT_INSTRUCTIONS.to_owned()
    }
    fn setup_hint(&self, ctx: &TaskContext<'_>) -> Option<String> {
        if !ctx.config.features.docs.enabled {
            return Some("needs [features.docs] enabled = true".to_owned());
        }
        (!crate::core::HealPaths::new(ctx.project).concepts().exists())
            .then(|| "no .heal/concepts.toml yet; run /heal-concepts-setup".to_owned())
    }
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let Some(concepts) = load_concepts(ctx)? else {
            return Ok(Vec::new());
        };
        let labels = concepts.labels();
        let vocab: String = labels.iter().map(|(k, v)| format!("{k}: {v}\n")).collect();
        let mut groups = Vec::new();
        for doc in doc_files(ctx) {
            let Some(body) = ctx.read_sendable(&doc) else {
                continue;
            };
            let secs = doc_sections(&body);
            if secs.is_empty() {
                continue;
            }
            let spans: Vec<(u32, u32)> = secs.iter().map(|s| (s.start_line, s.end_line)).collect();
            for (state, covered) in file_states(&doc, &body, &spans) {
                let state_key = format!("{state}\n--\n{vocab}");
                let items = covered
                    .iter()
                    .map(|&i| {
                        let s = &secs[i];
                        let subject = format!("\"{}\" (lines {}–{})", s.title, s.start_line, s.end_line);
                        Item {
                            key: ctx.key(self, &subject, &state_key),
                            question: Question::Choice {
                                instructions: json!(format!("{DOC_CONCEPT_INSTRUCTIONS}\nSection: {subject}")),
                                criteria: criteria(&labels),
                            },
                            meta: json!({"doc": doc.to_string_lossy(), "title": s.title, "start": s.start_line, "end": s.end_line}),
                        }
                    })
                    .collect();
                groups.push(Group {
                    items,
                    state: json!(state),
                });
            }
        }
        Ok(groups)
    }
    fn lower(&self, ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        let mut documented: BTreeSet<String> = BTreeSet::new();
        for a in answered {
            if let Some((c, p, _)) = a.answer.and_then(chosen) {
                if p >= 0.5 && c != OTHER {
                    documented.insert(c.to_owned());
                }
            }
        }
        // Code LOC per concept, and the file holding most of it.
        let mut code: BTreeMap<String, BTreeMap<PathBuf, u32>> = BTreeMap::new();
        let Ok(groups) = ConceptTask.plan(ctx) else {
            return Lowered::default();
        };
        for item in groups.iter().flat_map(|g| &g.items) {
            let Some((c, p, _)) = ctx.prior_answer("concept", &item.key).and_then(chosen) else {
                continue;
            };
            if p < 0.5 || c == OTHER {
                continue;
            }
            let start = item.meta["start"].as_u64().unwrap_or(0);
            let end = item.meta["end"].as_u64().unwrap_or(start);
            let loc = u32::try_from(end.saturating_sub(start) + 1).unwrap_or(0);
            *code
                .entry(c.to_owned())
                .or_default()
                .entry(PathBuf::from(item.meta["file"].as_str().unwrap_or("")))
                .or_insert(0) += loc;
        }
        let total: u32 = code.values().flat_map(BTreeMap::values).sum();
        let mut lowered = Lowered::default();
        if answered.iter().all(|a| a.answer.is_none()) {
            // Nothing classified yet: a gap would only mean "not asked".
            return lowered;
        }
        for (concept, files) in &code {
            let loc: u32 = files.values().sum();
            let share = f64::from(loc) / f64::from(total.max(1));
            if documented.contains(concept) || (share < GAP_SHARE && loc < GAP_MIN_LOC) {
                continue;
            }
            let Some((main, _)) = files.iter().max_by_key(|(_, l)| **l) else {
                continue;
            };
            let mut f = finding(
                "doc_concept.gap",
                main,
                None,
                None,
                format!("concept `{concept}` ({loc} lines of code, {:.0}% of the classified code) has no doc section explaining it", share * 100.0),
                &format!("doc_concept.gap:{concept}"),
                Severity::Medium,
            );
            f.fix_hint = Some(format!(
                "write an explanation or reference section for `{concept}`"
            ));
            lowered.findings.push(f);
        }
        lowered
    }
}

pub struct DocOverlap;

const OVERLAP_STATE_NOTE: &str = "The state shows two sections from different documentation pages that explain the same concept.";

impl Task for DocOverlap {
    fn id(&self) -> &'static str {
        "doc_overlap"
    }
    fn summary(&self) -> &'static str {
        "sections of different pages that repeat or contradict each other about one concept (needs `doc_concept`)"
    }
    fn depends_on(&self) -> &'static [&'static str] {
        &["doc_concept"]
    }
    fn criteria_text(&self) -> String {
        OVERLAP_STATE_NOTE.to_owned()
    }
    fn setup_hint(&self, _ctx: &TaskContext<'_>) -> Option<String> {
        Some(
            "needs `doc_concept` verdicts first; run `heal semantic ask --task doc_concept`"
                .to_owned(),
        )
    }
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        // concept → sections (doc, title, start, end)
        let mut by: BTreeMap<String, Vec<(PathBuf, String, u32, u32)>> = BTreeMap::new();
        for g in DocConcept.plan(ctx)? {
            for item in g.items {
                let Some((c, p, _)) = ctx.prior_answer("doc_concept", &item.key).and_then(chosen)
                else {
                    continue;
                };
                if p < 0.5 || c == OTHER {
                    continue;
                }
                let m = &item.meta;
                let (start, end) = (
                    u32::try_from(m["start"].as_u64().unwrap_or(0)).unwrap_or(0),
                    u32::try_from(m["end"].as_u64().unwrap_or(0)).unwrap_or(0),
                );
                if end + 1 - start < MIN_OVERLAP_LINES {
                    continue;
                }
                by.entry(c.to_owned()).or_default().push((
                    PathBuf::from(m["doc"].as_str().unwrap_or("")),
                    m["title"].as_str().unwrap_or("").to_owned(),
                    start,
                    end,
                ));
            }
        }
        let mut groups = Vec::new();
        for (concept, mut secs) in by {
            secs.sort_by(|a, b| {
                (b.3 - b.2)
                    .cmp(&(a.3 - a.2))
                    .then_with(|| (&a.0, a.2).cmp(&(&b.0, b.2)))
            });
            let mut n = 0;
            'pairs: for i in 0..secs.len() {
                for j in i + 1..secs.len() {
                    if secs[i].0 == secs[j].0 {
                        continue;
                    }
                    if n >= MAX_OVERLAP_PAIRS_PER_CONCEPT {
                        break 'pairs;
                    }
                    n += 1;
                    let (a, b) = (&secs[i], &secs[j]);
                    let text = |s: &(PathBuf, String, u32, u32)| {
                        ctx.read_sendable(&s.0)
                            .map(|body| numbered_range(&body, s.2, s.3))
                            .unwrap_or_default()
                    };
                    let state = format!(
                        "{OVERLAP_STATE_NOTE} Concept: `{concept}`.\n\nA — {} \"{}\":\n{}\nB — {} \"{}\":\n{}",
                        a.0.display(), a.1, text(a), b.0.display(), b.1, text(b)
                    );
                    let meta = |q: &str| json!({"q": q, "concept": concept, "a": a.0.to_string_lossy(), "a_line": a.2, "b": b.0.to_string_lossy(), "b_line": b.2, "a_title": a.1, "b_title": b.1});
                    groups.push(Group {
                        items: vec![
                            Item {
                                key: ctx.key(self, "duplicate", &state),
                                question: Question::Noul {
                                    instructions: json!("Section B explains the same thing as section A, so a reader gains nothing from reading both."),
                                    criteria: Some(NoulCriteria { yes: json!("Same explanation, reworded or repeated."), no: json!("They cover different aspects or audiences.") }),
                                },
                                meta: meta("duplicate"),
                            },
                            Item {
                                key: ctx.key(self, "conflict", &state),
                                question: Question::Noul {
                                    instructions: json!("Sections A and B make claims that contradict each other."),
                                    criteria: Some(NoulCriteria { yes: json!("They contradict each other on at least one point."), no: json!("They are consistent.") }),
                                },
                                meta: meta("conflict"),
                            },
                        ],
                        state: json!(state),
                    });
                }
            }
        }
        Ok(groups)
    }
    fn lower(&self, ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        let cutoff = ctx.cutoff(self, 0.7);
        let mut lowered = Lowered::default();
        for a in answered {
            let Some(p) = a.answer.and_then(noul_p) else {
                continue;
            };
            if p < cutoff {
                continue;
            }
            let m = &a.item.meta;
            let q = m["q"].as_str().unwrap_or("");
            let (da, db) = (m["a"].as_str().unwrap_or(""), m["b"].as_str().unwrap_or(""));
            let (ta, tb) = (
                m["a_title"].as_str().unwrap_or(""),
                m["b_title"].as_str().unwrap_or(""),
            );
            let concept = m["concept"].as_str().unwrap_or("");
            let (metric, summary, hint, sev) = if q == "conflict" {
                (
                    "doc_concept.conflict",
                    format!("\"{ta}\" contradicts \"{tb}\" in `{db}` about `{concept}`"),
                    "check both against the code (doc_drift.semantic helps) and fix the wrong one",
                    Severity::High,
                )
            } else {
                (
                    "doc_concept.duplicate",
                    format!(
                        "\"{ta}\" and \"{tb}\" in `{db}` explain the same thing about `{concept}`"
                    ),
                    "keep one explanation and link to it from the other page",
                    Severity::Medium,
                )
            };
            let mut f = finding(
                metric,
                Path::new(da),
                u32::try_from(m["a_line"].as_u64().unwrap_or(0)).ok(),
                Some(ta),
                summary,
                &format!("{metric}:{concept}:{ta}:{db}:{tb}"),
                sev,
            )
            .with_locations(vec![crate::core::finding::Location {
                file: PathBuf::from(db),
                line: u32::try_from(m["b_line"].as_u64().unwrap_or(0)).ok(),
                symbol: Some(tb.to_owned()),
            }]);
            f.fix_hint = Some(hint.to_owned());
            lowered.findings.push(f);
        }
        lowered
    }
}

pub struct DocPairs;

const PAIRS_INSTRUCTIONS: &str = "The state is the start of one documentation page. Judge whether the page documents the behaviour implemented in the named source file. A page may document several files.";
const MAX_PAIR_CANDIDATES: usize = 20;

fn path_words(p: &Path) -> BTreeSet<String> {
    p.to_string_lossy()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(str::to_lowercase)
        .collect()
}

impl Task for DocPairs {
    fn id(&self) -> &'static str {
        "doc_pairs"
    }
    fn summary(&self) -> &'static str {
        "on demand: for docs without a pair, which source file each documents (for /heal-doc-pair-setup)"
    }
    fn on_demand(&self) -> bool {
        true
    }
    fn criteria_text(&self) -> String {
        PAIRS_INSTRUCTIONS.to_owned()
    }
    fn setup_hint(&self, ctx: &TaskContext<'_>) -> Option<String> {
        (!ctx.config.features.docs.enabled)
            .then(|| "needs [features.docs] enabled = true".to_owned())
    }
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let paired: BTreeSet<String> = ctx
            .reports
            .and_then(|r| r.doc_pairs.as_ref())
            .map(|p| p.pairs.iter().map(|x| x.doc.clone()).collect())
            .unwrap_or_default();
        let srcs = code_files(ctx).0;
        let mut groups = Vec::new();
        for doc in doc_files(ctx) {
            if paired.contains(doc.to_string_lossy().as_ref()) {
                continue;
            }
            let Some(body) = ctx.read_sendable(&doc) else {
                continue;
            };
            let head = numbered_range(&body, 1, 120);
            let words: BTreeSet<String> = path_words(&doc)
                .into_iter()
                .chain(
                    head.split(|c: char| !c.is_alphanumeric() && c != '_')
                        .filter(|w| w.len() >= 4)
                        .map(str::to_lowercase),
                )
                .collect();
            let mut scored: Vec<(usize, &PathBuf)> = srcs
                .iter()
                .map(|s| (path_words(s).intersection(&words).count(), s))
                .filter(|(n, _)| *n > 0)
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
            if scored.is_empty() {
                continue;
            }
            // One yes/no per candidate, not a single choice: a page often
            // documents several files, and "pick the one file" drove the
            // live model to `none` on every page of this repository.
            let state = format!("Page: {}\n\n{head}", doc.display());
            let items = scored
                .into_iter()
                .take(MAX_PAIR_CANDIDATES)
                .map(|(_, src)| {
                    let src = src.to_string_lossy().into_owned();
                    Item {
                        key: ctx.key(self, &src, &state),
                        question: Question::Noul {
                            instructions: json!(format!(
                                "{PAIRS_INSTRUCTIONS}\nSource file: `{src}`"
                            )),
                            criteria: Some(NoulCriteria {
                                yes: json!(format!(
                                    "The page explains behaviour that `{src}` implements."
                                )),
                                no: json!(format!("The page does not describe what `{src}` does.")),
                            }),
                        },
                        meta: json!({"doc": doc.to_string_lossy(), "src": src}),
                    }
                })
                .collect();
            groups.push(Group {
                items,
                state: json!(state),
            });
        }
        Ok(groups)
    }
    /// One entry per doc, shaped like a `doc_pairs.json` pair: every
    /// candidate at or above the cutoff in `srcs` (most likely first),
    /// `confidence` = the lowest of their probabilities, and `scores` with
    /// each listed source's probability.
    fn report(&self, ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Option<Value> {
        let cutoff = ctx.cutoff(self, 0.6);
        let mut by_doc: BTreeMap<&str, Vec<(&str, f64)>> = BTreeMap::new();
        for a in answered {
            let Some(p) = a.answer.and_then(noul_p) else {
                continue;
            };
            let (Some(doc), Some(src)) = (a.item.meta["doc"].as_str(), a.item.meta["src"].as_str())
            else {
                continue;
            };
            if p >= cutoff {
                by_doc.entry(doc).or_default().push((src, p));
            }
        }
        let rows: Vec<Value> = by_doc
            .into_iter()
            .map(|(doc, mut srcs)| {
                srcs.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(b.0)));
                let confidence = srcs.iter().map(|(_, p)| *p).fold(1.0_f64, f64::min);
                json!({
                    "doc": doc,
                    "srcs": srcs.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
                    "confidence": confidence,
                    "scores": srcs.iter().map(|(s, p)| ((*s).to_owned(), json!(p))).collect::<serde_json::Map<_, _>>(),
                    "source": "llm",
                })
            })
            .collect();
        Some(json!({"pairs": rows, "cutoff": cutoff}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::tasks::testing::noul as answer_noul;

    #[test]
    fn doc_pairs_report_keeps_every_source_above_the_cutoff() {
        let cfg = crate::core::config::Config::default();
        let ctx = TaskContext::new(Path::new("."), &cfg).unwrap();
        let item = |doc: &str, src: &str| Item {
            key: format!("{doc}|{src}"),
            question: Question::Noul {
                instructions: json!(""),
                criteria: None,
            },
            meta: json!({"doc": doc, "src": src}),
        };
        // Probabilities measured on this repository's test metrics page.
        let items = [
            item("docs/test/metrics.md", "src/test/coverage.rs"),
            item("docs/test/metrics.md", "src/test/skip_ratio.rs"),
            item("docs/test/metrics.md", "src/test/hotspot.rs"),
            item("docs/test/metrics.md", "src/code/lcom.rs"),
            item("docs/other.md", "src/auth.rs"),
        ];
        let answers = [0.67, 0.83, 0.75, 0.39, 0.01].map(answer_noul);
        let answered: Vec<Answered<'_>> = items
            .iter()
            .zip(&answers)
            .map(|(item, a)| Answered {
                item,
                answer: Some(a),
            })
            .collect();
        let report = DocPairs.report(&ctx, &answered).unwrap();
        let pairs = report["pairs"].as_array().unwrap();
        assert_eq!(pairs.len(), 1, "{report}");
        assert_eq!(pairs[0]["doc"], "docs/test/metrics.md");
        assert_eq!(
            pairs[0]["srcs"],
            json!([
                "src/test/skip_ratio.rs",
                "src/test/hotspot.rs",
                "src/test/coverage.rs"
            ])
        );
        assert!((pairs[0]["confidence"].as_f64().unwrap() - 0.67).abs() < 1e-9);
        assert_eq!(pairs[0]["source"], "llm");
    }

    #[test]
    fn overlap_lowers_duplicates_and_conflicts() {
        let cfg = crate::core::config::Config::default();
        let ctx = TaskContext::new(Path::new("."), &cfg).unwrap();
        let meta = |q: &str| json!({"q": q, "concept": "cache", "a": "a.md", "a_line": 3, "b": "b.md", "b_line": 9, "a_title": "Cache", "b_title": "Caching"});
        let items = [
            Item {
                key: "d".into(),
                question: Question::Noul {
                    instructions: json!(""),
                    criteria: None,
                },
                meta: meta("duplicate"),
            },
            Item {
                key: "c".into(),
                question: Question::Noul {
                    instructions: json!(""),
                    criteria: None,
                },
                meta: meta("conflict"),
            },
        ];
        let answers = [answer_noul(0.9), answer_noul(0.8)];
        let answered: Vec<Answered<'_>> = items
            .iter()
            .zip(&answers)
            .map(|(item, a)| Answered {
                item,
                answer: Some(a),
            })
            .collect();
        let lowered = DocOverlap.lower(&ctx, &answered);
        let got: Vec<(&str, Severity)> = lowered
            .findings
            .iter()
            .map(|f| (f.metric.as_str(), f.severity))
            .collect();
        assert_eq!(
            got,
            [
                ("doc_concept.duplicate", Severity::Medium),
                ("doc_concept.conflict", Severity::High)
            ]
        );
        assert_eq!(lowered.findings[0].locations[0].file, PathBuf::from("b.md"));
    }

    #[test]
    fn path_words_split_on_separators() {
        assert_eq!(
            path_words(Path::new("docs/semantic-tasks/overview.md")),
            ["docs", "overview", "semantic", "tasks"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect()
        );
    }
}
