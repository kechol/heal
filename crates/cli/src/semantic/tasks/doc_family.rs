//! Docs-family tasks. All need `[features.docs] enabled = true`.
//!
//! - `doc_structure` — docjev's classify-and-split
//!   (jerryjliu/docjev, Apache-2.0) applied to Markdown: every section is
//!   classified by document kind (the four Diátaxis modes plus changelog,
//!   ADR, runbook, glossary), and every section boundary is asked
//!   whether a new document starts there. A page holding several
//!   documents → `doc_structure.split`; one document drifting between
//!   modes → `doc_structure.mixed_mode`; small neighbouring pages that
//!   continue one document → `doc_structure.merge`. Boundaries within
//!   0.1 of the threshold are flagged for review rather than acted on,
//!   as docjev does.
//! - `doc_placement` — which section of the doc tree each page
//!   belongs under; a page filed elsewhere → `doc_placement`. The chosen
//!   section also becomes the registration slot for `orphan_pages`.
//! - `doc_drift_semantic` — for each paired doc section and its
//!   sources: does the section state something the code no longer does?
//!   Type 3 drift in `.claude/docs/observers.md` terms →
//!   `doc_drift.semantic`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::core::finding::SemanticNote;
use crate::core::severity::Severity;
use crate::feature::Family;
use crate::observer::docs::sections::{sections, Section};
use crate::semantic::api::{NoulCriteria, Question};
use crate::semantic::task::{Answered, Group, Item, Lowered, Task, TaskContext};
use crate::semantic::tasks::common::{
    applicable_share, chosen, criteria, finding, labels, note, noul_p, numbered, numbered_range,
};

fn docs_hint(ctx: &TaskContext<'_>) -> Option<String> {
    (!ctx.config.features.docs.enabled).then(|| "needs [features.docs] enabled = true".to_owned())
}

/// Markdown docs HEAL observes: standalone (Layer B) and paired (Layer A),
/// sendable, sorted.
pub(crate) fn doc_files(ctx: &TaskContext<'_>) -> Vec<PathBuf> {
    if !ctx.config.features.docs.enabled {
        return Vec::new();
    }
    let mut out: BTreeSet<PathBuf> =
        crate::observer::docs::walk::walk_standalone_docs(ctx.project, ctx.config)
            .into_iter()
            .collect();
    if let Some(pairs) = ctx.reports.and_then(|r| r.doc_pairs.as_ref()) {
        for p in pairs.live_pairs(ctx.project) {
            out.insert(PathBuf::from(&p.doc));
        }
    }
    out.into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "md" || e == "mdx") && ctx.may_send(p))
        .collect()
}

fn body_sections(body: &str) -> Vec<Section> {
    sections(body)
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect()
}

fn line_count(s: &Section) -> u32 {
    s.end_line - s.start_line + 1
}

// ------------------------------------------------------------ doc_structure

pub const DOC_KINDS: [(&str, &str); 9] = [
    (
        "tutorial",
        "A lesson that walks a newcomer through a first success, step by step.",
    ),
    (
        "how_to",
        "Directions for accomplishing one specific task the reader already has in mind.",
    ),
    (
        "reference",
        "Neutral description of the machinery: options, fields, commands, APIs, facts to look up.",
    ),
    (
        "explanation",
        "Discussion of background, design, and reasons: why things are the way they are.",
    ),
    ("changelog", "A record of changes between versions."),
    (
        "adr",
        "An architecture decision record: context, decision, consequences.",
    ),
    (
        "runbook",
        "Operational procedure for running or recovering a system.",
    ),
    ("glossary", "Definitions of terms."),
    (
        "other",
        "Anything else (navigation, landing page, legal text).",
    ),
];
const KIND_INSTRUCTIONS: &str = "The state is a documentation page with line numbers. Decide what kind of document the named section is.";
const BOUNDARY_INSTRUCTIONS: &str = "The state is a documentation page with line numbers. Decide whether the named section starts a separate document from the section before it: a different purpose or a different reader, even if the kind is the same.";
const MERGE_INSTRUCTIONS: &str = "The state shows two short documentation pages from the same directory. Decide whether page B continues the same document as page A, so that the two would read better as one page.";
/// Boundary probability at which a new document starts.
const SPLIT_CUTOFF: f64 = 0.7;
/// Boundaries within this distance of the cutoff are flagged for review.
const REVIEW_MARGIN: f64 = 0.1;
const MAX_SECTIONS_PER_DOC: usize = 60;
const MERGE_MAX_LINES: usize = 60;
const MAX_MERGE_PAIRS_PER_DIR: usize = 10;

pub struct DocStructure;

impl Task for DocStructure {
    fn id(&self) -> &'static str {
        "doc_structure"
    }
    fn summary(&self) -> &'static str {
        "classify every doc section (Diátaxis + changelog / ADR / runbook / glossary); find pages to split, merge, or restructure"
    }
    fn criteria_text(&self) -> String {
        let mut s = format!("{KIND_INSTRUCTIONS}\n{BOUNDARY_INSTRUCTIONS}\n{MERGE_INSTRUCTIONS}");
        for (k, v) in DOC_KINDS {
            s.push_str(&format!("\n{k}: {v}"));
        }
        s
    }
    fn setup_hint(&self, ctx: &TaskContext<'_>) -> Option<String> {
        docs_hint(ctx)
    }
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let labels = labels(&DOC_KINDS);
        let mut groups = Vec::new();
        let mut small_by_dir: BTreeMap<PathBuf, Vec<(PathBuf, String)>> = BTreeMap::new();
        for doc in doc_files(ctx) {
            let Some(body) = ctx.read_sendable(&doc) else {
                continue;
            };
            if body.lines().count() <= MERGE_MAX_LINES {
                small_by_dir
                    .entry(doc.parent().map(Path::to_path_buf).unwrap_or_default())
                    .or_default()
                    .push((doc.clone(), body.clone()));
            }
            let secs: Vec<Section> = body_sections(&body)
                .into_iter()
                .take(MAX_SECTIONS_PER_DOC)
                .collect();
            if secs.is_empty() {
                continue;
            }
            let spans: Vec<(u32, u32)> = secs.iter().map(|s| (s.start_line, s.end_line)).collect();
            for (state, covered) in crate::semantic::tasks::common::file_states(&doc, &body, &spans)
            {
                let mut items = Vec::new();
                for &i in &covered {
                    let s = &secs[i];
                    let subject =
                        format!("\"{}\" (lines {}–{})", s.title, s.start_line, s.end_line);
                    let meta = |q: &str| json!({"doc": doc.to_string_lossy(), "section": i, "title": s.title, "start": s.start_line, "end": s.end_line, "q": q});
                    items.push(Item {
                        key: ctx.key(self, &format!("kind:{subject}"), &state),
                        question: Question::Choice {
                            instructions: json!(format!("{KIND_INSTRUCTIONS}\nSection: {subject}")),
                            criteria: criteria(&labels),
                        },
                        meta: meta("kind"),
                    });
                    if i > 0 {
                        items.push(Item {
                            key: ctx.key(self, &format!("boundary:{subject}"), &state),
                            question: Question::Noul {
                                instructions: json!(format!(
                                    "{BOUNDARY_INSTRUCTIONS}\nSection: {subject}"
                                )),
                                criteria: Some(NoulCriteria {
                                    yes: json!("A separate document starts here."),
                                    no: json!("It continues the same document."),
                                }),
                            },
                            meta: meta("boundary"),
                        });
                    }
                }
                groups.push(Group {
                    items,
                    state: json!(state),
                });
            }
        }
        for (dir, pages) in small_by_dir {
            for w in pages.windows(2).take(MAX_MERGE_PAIRS_PER_DIR) {
                let ((a, ba), (b, bb)) = (&w[0], &w[1]);
                let state = format!(
                    "Page A: {}\n{}\nPage B: {}\n{}",
                    a.display(),
                    numbered(ba),
                    b.display(),
                    numbered(bb)
                );
                groups.push(Group {
                    items: vec![Item {
                        key: ctx.key(self, "merge", &state),
                        question: Question::Noul {
                            instructions: json!(MERGE_INSTRUCTIONS),
                            criteria: Some(NoulCriteria {
                                yes: json!("B continues A; they are one document."),
                                no: json!("They are separate documents."),
                            }),
                        },
                        meta: json!({"q": "merge", "a": a.to_string_lossy(), "b": b.to_string_lossy(), "dir": dir.to_string_lossy()}),
                    }],
                    state: json!(state),
                });
            }
        }
        Ok(groups)
    }
    #[allow(clippy::too_many_lines)] // one pass: merge pairs, then per-page segments; splitting scatters the segment state
    fn lower(&self, ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        // doc → section index → (title, start, end, kind, boundary p)
        type Sec = (String, u32, u32, Option<(String, f64)>, Option<f64>);
        let mut docs: BTreeMap<String, BTreeMap<u64, Sec>> = BTreeMap::new();
        let mut lowered = Lowered::default();
        for a in answered {
            let Some(answer) = a.answer else { continue };
            let m = &a.item.meta;
            if m["q"] == "merge" {
                if noul_p(answer).is_some_and(|p| p >= SPLIT_CUTOFF) {
                    let (pa, pb) = (m["a"].as_str().unwrap_or(""), m["b"].as_str().unwrap_or(""));
                    let mut f = finding(
                        "doc_structure.merge",
                        Path::new(pa),
                        None,
                        None,
                        format!("`{pb}` continues the same document as this page"),
                        &format!("doc_structure.merge:{pb}"),
                        Severity::Medium,
                    )
                    .with_locations(vec![
                        crate::core::finding::Location::file(PathBuf::from(pb)),
                    ]);
                    f.fix_hint = Some(format!(
                        "merge `{pb}` into this page and redirect its links"
                    ));
                    lowered.findings.push(f);
                }
                continue;
            }
            let doc = m["doc"].as_str().unwrap_or("").to_owned();
            let idx = m["section"].as_u64().unwrap_or(0);
            let e = docs.entry(doc).or_default().entry(idx).or_insert_with(|| {
                (
                    m["title"].as_str().unwrap_or("").to_owned(),
                    u32::try_from(m["start"].as_u64().unwrap_or(0)).unwrap_or(0),
                    u32::try_from(m["end"].as_u64().unwrap_or(0)).unwrap_or(0),
                    None,
                    None,
                )
            });
            if m["q"] == "kind" {
                e.3 = chosen(answer).map(|(l, p, _)| (l.to_owned(), p));
            } else {
                e.4 = noul_p(answer);
            }
        }
        for (doc, secs) in docs {
            let secs: Vec<&Sec> = secs.values().collect();
            // Segments split at confident boundaries.
            let mut segments: Vec<Vec<&Sec>> = vec![Vec::new()];
            let mut review = Vec::new();
            for (i, s) in secs.iter().enumerate() {
                if i > 0 {
                    if let Some(p) = s.4 {
                        if (p - SPLIT_CUTOFF).abs() <= REVIEW_MARGIN {
                            review.push(s.1);
                        }
                        if p >= SPLIT_CUTOFF {
                            segments.push(Vec::new());
                        }
                    }
                }
                segments.last_mut().expect("non-empty").push(s);
            }
            let kind_of = |seg: &[&Sec]| -> String {
                let mut by: BTreeMap<&str, u32> = BTreeMap::new();
                for s in seg {
                    if let Some((k, _)) = &s.3 {
                        *by.entry(k.as_str()).or_default() += s.2 - s.1 + 1;
                    }
                }
                by.into_iter()
                    .max_by_key(|(_, n)| *n)
                    .map_or_else(|| "other".to_owned(), |(k, _)| k.to_owned())
            };
            let review_detail = (!review.is_empty()).then(|| {
                format!(
                    "review: boundary before line {} is close to the threshold",
                    review
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            });
            let doc_path = PathBuf::from(&doc);
            if segments.len() >= 2 {
                let parts: Vec<String> = segments
                    .iter()
                    .filter(|seg| !seg.is_empty())
                    .map(|seg| {
                        format!(
                            "lines {}–{} ({})",
                            seg[0].1,
                            seg[seg.len() - 1].2,
                            kind_of(seg)
                        )
                    })
                    .collect();
                let mut f = finding(
                    "doc_structure.split",
                    &doc_path,
                    Some(segments[1].first().map_or(1, |s| s.1)),
                    None,
                    format!(
                        "{} documents in one page: {}",
                        parts.len(),
                        parts.join(", ")
                    ),
                    &format!("doc_structure.split:{}", parts.len()),
                    Severity::Medium,
                );
                f.fix_hint = Some(
                    "split the page along these ranges, one document per page, and link them"
                        .to_owned(),
                );
                f.semantic.insert(
                    "doc_structure".to_owned(),
                    SemanticNote {
                        label: "split".to_owned(),
                        p: 1.0,
                        confidence: 1.0,
                        lines: segments
                            .iter()
                            .skip(1)
                            .filter_map(|seg| seg.first().map(|s| s.1))
                            .collect(),
                        detail: review_detail.clone(),
                    },
                );
                lowered.findings.push(f);
            } else {
                // One document: flag substantial sections of different kinds.
                let total: u32 = secs.iter().map(|s| s.2 - s.1 + 1).sum();
                let mut by: BTreeMap<&str, u32> = BTreeMap::new();
                for s in &secs {
                    if let Some((k, p)) = &s.3 {
                        if *p >= 0.5 && k != "other" {
                            *by.entry(k.as_str()).or_default() += s.2 - s.1 + 1;
                        }
                    }
                }
                let big: Vec<(&str, u32)> = by
                    .into_iter()
                    .filter(|(_, n)| f64::from(*n) >= 0.25 * f64::from(total.max(1)))
                    .collect();
                if big.len() >= 2 {
                    let desc: Vec<String> = big
                        .iter()
                        .map(|(k, n)| {
                            format!(
                                "{k} ({:.0}%)",
                                f64::from(*n) * 100.0 / f64::from(total.max(1))
                            )
                        })
                        .collect();
                    let ids: Vec<&str> = big.iter().map(|(k, _)| *k).collect();
                    let mut f = finding(
                        "doc_structure.mixed_mode",
                        &doc_path,
                        None,
                        None,
                        format!("one document mixes modes: {}", desc.join(", ")),
                        &format!("doc_structure.mixed_mode:{}", ids.join("+")),
                        Severity::Medium,
                    );
                    f.fix_hint = Some("keep one mode per page: move the other mode's sections to their own page (Diátaxis)".to_owned());
                    lowered.findings.push(f);
                }
            }
            // The page's kind on its existing docs-family findings.
            let page_kind = kind_of(&secs);
            for f in ctx.findings.iter().filter(|f| {
                f.location.file == doc_path && Family::for_metric(&f.metric) == Family::Docs
            }) {
                lowered.notes.push((
                    f.id.clone(),
                    "doc_kind".to_owned(),
                    SemanticNote {
                        label: page_kind.clone(),
                        p: 1.0,
                        confidence: 1.0,
                        lines: Vec::new(),
                        detail: None,
                    },
                ));
            }
        }
        lowered
    }
}

// ------------------------------------------------------------ doc_placement

pub struct DocPlacement;

const PLACEMENT_INSTRUCTIONS: &str = "The state is one documentation page. Choose the section of the documentation tree where a reader would expect to find it.";
const MIN_P_GAIN: f64 = 0.2;

/// Candidate sections: every directory holding a doc, described by its
/// index page's first lines or by the titles of its pages.
fn tree_sections(ctx: &TaskContext<'_>, docs: &[PathBuf]) -> Vec<(String, String)> {
    let mut by_dir: BTreeMap<PathBuf, Vec<&PathBuf>> = BTreeMap::new();
    for d in docs {
        by_dir
            .entry(d.parent().map(Path::to_path_buf).unwrap_or_default())
            .or_default()
            .push(d);
    }
    by_dir
        .into_iter()
        .take(crate::semantic::api::Question::MAX_CHOICE_OPTIONS)
        .map(|(dir, pages)| {
            let index = pages.iter().find(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "index" | "readme"))
            });
            let desc = index.and_then(|p| ctx.read_sendable(p)).map_or_else(
                || {
                    let titles: Vec<String> = pages
                        .iter()
                        .take(6)
                        .filter_map(|p| ctx.read_sendable(p))
                        .filter_map(|b| {
                            body_sections(&b)
                                .into_iter()
                                .find(|s| s.level > 0)
                                .map(|s| s.title)
                        })
                        .collect();
                    format!("Pages: {}", titles.join("; "))
                },
                |b| {
                    b.lines()
                        .filter(|l| !l.trim().is_empty())
                        .take(6)
                        .collect::<Vec<_>>()
                        .join(" ")
                },
            );
            let id = if dir.as_os_str().is_empty() {
                "./".to_owned()
            } else {
                format!("{}/", dir.display())
            };
            (id, desc.chars().take(400).collect())
        })
        .collect()
}

impl Task for DocPlacement {
    fn id(&self) -> &'static str {
        "doc_placement"
    }
    fn summary(&self) -> &'static str {
        "which section of the doc tree each page belongs under (moves, and where to link orphans)"
    }
    fn criteria_text(&self) -> String {
        PLACEMENT_INSTRUCTIONS.to_owned()
    }
    fn setup_hint(&self, ctx: &TaskContext<'_>) -> Option<String> {
        docs_hint(ctx)
    }
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let docs = doc_files(ctx);
        let sections = tree_sections(ctx, &docs);
        if sections.len() < 2 {
            return Ok(Vec::new());
        }
        let tree: String = sections
            .iter()
            .map(|(k, v)| format!("{k}: {v}\n"))
            .collect();
        let mut groups = Vec::new();
        for doc in docs {
            let Some(body) = ctx.read_sendable(&doc) else {
                continue;
            };
            let state = format!(
                "Page: {}\n\n{}",
                doc.display(),
                numbered_range(&body, 1, 150)
            );
            groups.push(Group {
                items: vec![Item {
                    key: ctx.key(self, &tree, &state),
                    question: Question::Choice {
                        instructions: json!(PLACEMENT_INSTRUCTIONS),
                        criteria: criteria(&sections),
                    },
                    meta: json!({"doc": doc.to_string_lossy()}),
                }],
                state: json!(state),
            });
        }
        Ok(groups)
    }
    fn lower(&self, ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        let cutoff = ctx.cutoff(self, 0.6);
        let mut lowered = Lowered::default();
        for a in answered {
            let Some(answer) = a.answer else { continue };
            let Some((best, p, conf)) = chosen(answer) else {
                continue;
            };
            let doc = PathBuf::from(a.item.meta["doc"].as_str().unwrap_or(""));
            let current = doc.parent().map_or_else(
                || "./".to_owned(),
                |d| {
                    if d.as_os_str().is_empty() {
                        "./".to_owned()
                    } else {
                        format!("{}/", d.display())
                    }
                },
            );
            let p_current = match answer {
                crate::semantic::api::Answer::Choice { probabilities, .. } => {
                    probabilities.get(&current).copied().unwrap_or(0.0)
                }
                _ => 0.0,
            };
            // Orphans get the chosen section as their link slot.
            for f in ctx
                .findings
                .iter()
                .filter(|f| f.location.file == doc && f.metric == "orphan_pages")
            {
                let mut n = note(best, answer, 0);
                n.detail = Some(format!("link this page from `{best}`"));
                lowered
                    .notes
                    .push((f.id.clone(), "placement".to_owned(), n));
            }
            if best != current && p >= cutoff && p - p_current >= MIN_P_GAIN {
                let mut f = finding(
                    "doc_placement",
                    &doc,
                    None,
                    None,
                    format!("this page reads as part of `{best}`, not `{current}`"),
                    &format!("doc_placement:{best}"),
                    Severity::Medium,
                );
                f.fix_hint = Some(format!(
                    "move it under `{best}` and update the links and navigation"
                ));
                f.semantic.insert(
                    "doc_placement".to_owned(),
                    SemanticNote {
                        label: best.to_owned(),
                        p,
                        confidence: conf,
                        lines: Vec::new(),
                        detail: Some(format!("p(current)={p_current:.2}")),
                    },
                );
                lowered.findings.push(f);
            }
        }
        lowered
    }
}

// ------------------------------------------------------------------ doc_drift_semantic

pub struct DocDriftSemantic;

const DRIFT_INSTRUCTIONS: &str = "The state is a documentation page (or one of its sections) followed by the source code it documents, with line numbers. Judge whether the named section still describes what the code does.";
const DRIFT_LEVELS: [&str; 4] = [
    "Not applicable: the section does not describe this code's behaviour.",
    "Accurate: what the section says matches the code.",
    "Partly outdated: some details no longer match, but the gist holds.",
    "Wrong: the section states something the code no longer does, or misses a change a reader must know.",
];
const MAX_DRIFT_SECTIONS_PER_DOC: usize = 40;

impl Task for DocDriftSemantic {
    fn id(&self) -> &'static str {
        "doc_drift_semantic"
    }
    fn summary(&self) -> &'static str {
        "paired doc sections that state what the code no longer does (doc_drift Type 3)"
    }
    fn criteria_text(&self) -> String {
        format!("{DRIFT_INSTRUCTIONS}\n{}", DRIFT_LEVELS.join("\n"))
    }
    fn setup_hint(&self, ctx: &TaskContext<'_>) -> Option<String> {
        docs_hint(ctx).or_else(|| {
            ctx.reports
                .and_then(|r| r.doc_pairs.as_ref())
                .is_none()
                .then(|| "needs .heal/doc_pairs.json; run /heal:setup".to_owned())
        })
    }
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        if !ctx.config.features.docs.enabled {
            return Ok(Vec::new());
        }
        let Some(pairs) = ctx.reports.and_then(|r| r.doc_pairs.as_ref()) else {
            return Ok(Vec::new());
        };
        let budget = crate::semantic::cost::state_budget();
        let mut groups = Vec::new();
        for pair in pairs.live_pairs(ctx.project) {
            let doc = PathBuf::from(&pair.doc);
            let Some(body) = ctx.read_sendable(&doc) else {
                continue;
            };
            let mut code = String::new();
            for src in &pair.srcs {
                let Some(text) = ctx.read_sendable(Path::new(src)) else {
                    continue;
                };
                let block = format!("\nSource {src}:\n{}", numbered(&text));
                if crate::semantic::cost::estimate_tokens(code.len() + block.len())
                    <= budget * 3 / 4
                {
                    code.push_str(&block);
                }
            }
            if code.is_empty() {
                continue;
            }
            let secs: Vec<Section> = body_sections(&body)
                .into_iter()
                .filter(|s| s.level > 0)
                .take(MAX_DRIFT_SECTIONS_PER_DOC)
                .collect();
            let item = |s: &Section, state: &str| Item {
                key: ctx.key(self, &s.title, state),
                question: Question::Score {
                    instructions: json!(format!(
                        "{DRIFT_INSTRUCTIONS}\nSection: \"{}\" (lines {}–{})",
                        s.title, s.start_line, s.end_line
                    )),
                    criteria: DRIFT_LEVELS.iter().map(|l| json!(l)).collect(),
                },
                meta: json!({"doc": pair.doc, "title": s.title, "start": s.start_line, "lines": line_count(s)}),
            };
            // One state per pair (whole doc + code) when it fits; else one
            // state per section.
            let whole = format!("Doc {}:\n{}\n{code}", doc.display(), numbered(&body));
            if crate::semantic::cost::estimate_tokens(whole.len()) <= budget {
                let items = secs.iter().map(|s| item(s, &whole)).collect();
                groups.push(Group {
                    items,
                    state: json!(whole),
                });
                continue;
            }
            for s in &secs {
                let state = format!(
                    "Doc {} — section (lines {}–{}):\n{}\n{code}",
                    doc.display(),
                    s.start_line,
                    s.end_line,
                    numbered_range(&body, s.start_line, s.end_line)
                );
                if crate::semantic::cost::estimate_tokens(state.len()) > budget {
                    continue;
                }
                groups.push(Group {
                    items: vec![item(s, &state)],
                    state: json!(state),
                });
            }
        }
        Ok(groups)
    }
    fn lower(&self, ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        let cutoff = ctx.cutoff(self, 0.6);
        let mut lowered = Lowered::default();
        for a in answered {
            let Some(answer) = a.answer else { continue };
            // Level 0 is "not applicable", so read the probabilities, not
            // the mean `score` (see `applicable_share`). "Partly outdated"
            // and "wrong" both count; the larger one picks the wording.
            let levels = DRIFT_LEVELS.len();
            let Some((applies, stale)) = applicable_share(answer, levels, 2) else {
                continue;
            };
            if applies < 0.5 || stale < cutoff {
                continue;
            }
            let wrong = applicable_share(answer, levels, 3).map_or(0.0, |(_, w)| w);
            let what = if wrong * 2.0 >= stale {
                "states something the paired code no longer does"
            } else {
                "has details that no longer match the paired code"
            };
            let m = &a.item.meta;
            let title = m["title"].as_str().unwrap_or("");
            let doc = PathBuf::from(m["doc"].as_str().unwrap_or(""));
            let line = u32::try_from(m["start"].as_u64().unwrap_or(0)).ok();
            let mut f = finding(
                "doc_drift.semantic",
                &doc,
                line,
                Some(title),
                format!("section \"{title}\" {what}"),
                &format!("doc_drift.semantic:{title}"),
                Severity::Medium,
            );
            f.fix_hint = Some(
                "compare the section with the paired sources and rewrite what changed".to_owned(),
            );
            lowered.findings.push(f);
        }
        lowered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::api::Answer;
    use crate::semantic::tasks::testing::{choice, noul as answer_noul};

    fn sec_item(doc: &str, i: u64, start: u32, end: u32, q: &str) -> Item {
        Item {
            key: format!("{doc}{i}{q}"),
            question: Question::Noul {
                instructions: json!(""),
                criteria: None,
            },
            meta: json!({"doc": doc, "section": i, "title": format!("S{i}"), "start": start, "end": end, "q": q}),
        }
    }

    fn drift_answer(probs: &[f64; 4]) -> Answer {
        Answer::Score {
            score: probs
                .iter()
                .zip(0_u32..)
                .map(|(p, i)| f64::from(i) * p)
                .sum(),
            probabilities: probs
                .iter()
                .enumerate()
                .map(|(i, p)| (i.to_string(), *p))
                .collect(),
            confidence: 0.0,
        }
    }

    #[test]
    fn drift_reads_probabilities_not_the_mean_score() {
        let cfg = crate::core::config::Config::default();
        let ctx = TaskContext::new(Path::new("."), &cfg).unwrap();
        let item = |title: &str| Item {
            key: title.to_owned(),
            question: Question::Noul {
                instructions: json!(""),
                criteria: None,
            },
            meta: json!({"doc": "docs/a.md", "title": title, "start": 1, "lines": 10}),
        };
        let items = [
            item("split"),
            item("mostly_na"),
            item("partly"),
            item("fine"),
        ];
        // Measured shapes from the live API. `split` has mean 1.52, which
        // the old `score >= 2.5` rule read as "accurate".
        let answers = [
            drift_answer(&[0.34, 0.12, 0.20, 0.34]),
            drift_answer(&[0.61, 0.03, 0.01, 0.35]),
            drift_answer(&[0.05, 0.20, 0.70, 0.05]),
            drift_answer(&[0.10, 0.85, 0.04, 0.01]),
        ];
        let answered: Vec<Answered<'_>> = items
            .iter()
            .zip(&answers)
            .map(|(item, a)| Answered {
                item,
                answer: Some(a),
            })
            .collect();
        let lowered = DocDriftSemantic.lower(&ctx, &answered);
        let found: Vec<(&str, &str)> = lowered
            .findings
            .iter()
            .map(|f| {
                (
                    f.location.symbol.as_deref().unwrap_or(""),
                    f.summary.as_str(),
                )
            })
            .collect();
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].0, "split");
        assert!(found[0].1.contains("no longer does"), "{}", found[0].1);
        assert_eq!(found[1].0, "partly");
        assert!(found[1].1.contains("no longer match"), "{}", found[1].1);
    }

    #[test]
    fn split_and_mixed_mode() {
        let cfg = crate::core::config::Config::default();
        let ctx = TaskContext::new(Path::new("."), &cfg).unwrap();
        // Page a.md: two documents (boundary before section 2).
        // Page b.md: one document mixing how_to and reference.
        let items = [
            sec_item("a.md", 0, 1, 20, "kind"),
            sec_item("a.md", 1, 21, 40, "kind"),
            sec_item("a.md", 1, 21, 40, "boundary"),
            sec_item("a.md", 2, 41, 60, "kind"),
            sec_item("a.md", 2, 41, 60, "boundary"),
            sec_item("b.md", 0, 1, 30, "kind"),
            sec_item("b.md", 1, 31, 60, "kind"),
            sec_item("b.md", 1, 31, 60, "boundary"),
        ];
        let answers = [
            choice("how_to", 0.9),
            choice("how_to", 0.9),
            answer_noul(0.2),
            choice("reference", 0.9),
            answer_noul(0.9),
            choice("how_to", 0.9),
            choice("reference", 0.9),
            answer_noul(0.1),
        ];
        let answered: Vec<Answered<'_>> = items
            .iter()
            .zip(&answers)
            .map(|(item, a)| Answered {
                item,
                answer: Some(a),
            })
            .collect();
        let lowered = DocStructure.lower(&ctx, &answered);
        let metrics: Vec<&str> = lowered.findings.iter().map(|f| f.metric.as_str()).collect();
        assert_eq!(metrics, ["doc_structure.split", "doc_structure.mixed_mode"]);
        assert!(
            lowered.findings[0].summary.contains("lines 1–40 (how_to)"),
            "{}",
            lowered.findings[0].summary
        );
        assert!(lowered.findings[0]
            .summary
            .contains("lines 41–60 (reference)"));
    }
}
