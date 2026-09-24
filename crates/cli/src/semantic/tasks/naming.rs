//! Naming tasks.
//!
//! - `term_drift` — two words that name the same domain thing inside one
//!   concept (`user` / `account`). Built on the `concept` map: candidate
//!   pairs are the frequent nouns of one concept's function names that
//!   never appear together. Keeping one word per thing is the ubiquitous
//!   language of Domain-Driven Design (Evans), and the same rule HEAL
//!   applies to itself through `.claude/docs/glossary.md`.
//! - `name_mismatch` — a function in a hotspot file (or one with a CCN /
//!   Cognitive finding) whose name or doc comment claims something its
//!   body does not do. jev-lint's measurements put this class of question
//!   among the model's strongest.
//! - `name_choice` — on demand: given candidate names the agent wrote,
//!   which one best describes the function. Jev picks; it never invents.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::core::concepts::OTHER;
use crate::core::severity::Severity;
use crate::feature::Family;
use crate::semantic::api::{NoulCriteria, Question};
use crate::semantic::task::{Answered, Group, Item, Lowered, Task, TaskContext};
use crate::semantic::tasks::common::{
    applicable_share, chosen, code_files, criteria, file_facts, file_states, finding, noul_p,
};
use crate::semantic::tasks::concept::{load_concepts, ConceptTask};

/// Split an identifier into lowercase words (`snake_case`, `camelCase`,
/// `PascalCase`, `kebab-case`).
#[must_use]
pub fn words(ident: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for ch in ident.chars() {
        if !ch.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if ch.is_uppercase() && prev_lower && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
        cur.extend(ch.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Verbs and glue words that say what a function does, not what it is
/// about; they never form a term pair.
const NON_TERMS: &[&str] = &[
    "get", "set", "is", "has", "new", "from", "into", "to", "as", "with", "for", "of", "and", "or",
    "the", "a", "an", "by", "on", "in", "at", "load", "save", "read", "write", "parse", "build",
    "make", "create", "delete", "remove", "add", "update", "apply", "compute", "find", "run",
    "handle", "render", "check", "validate", "try", "do", "all", "each", "default", "test", "fmt",
    "impl", "init", "open", "close", "push", "pop", "emit", "collect", "map",
];

/// The singular form of a plural English word, so `function` /
/// `functions` or `count` / `counts` count as one term. Without it the
/// two forms of one word look like two terms that never share a name —
/// exactly what the pair question asks about — and Jev rightly answers
/// that they name the same thing. Deliberately simple: only regular
/// plurals, and words ending in `ss` / `us` / `is` / `as` (`class`,
/// `status`, `analysis`, `alias`) are left alone.
#[must_use]
pub fn singular(word: &str) -> String {
    let n = word.len();
    if n > 4 && word.ends_with("ies") {
        return format!("{}y", &word[..n - 3]);
    }
    for suffix in ["sses", "xes", "ches", "shes", "zes"] {
        if n > suffix.len() + 1 && word.ends_with(suffix) {
            return word[..n - 2].to_owned();
        }
    }
    let keep = ["ss", "us", "is", "as"];
    if n > 3 && word.ends_with('s') && !keep.iter().any(|k| word.ends_with(k)) {
        return word[..n - 1].to_owned();
    }
    word.to_owned()
}

/// The terms of one identifier: its words in singular form, minus verbs
/// and glue words and anything shorter than three letters.
#[must_use]
pub fn term_words(symbol: &str) -> BTreeSet<String> {
    words(symbol)
        .into_iter()
        .map(|w| singular(&w))
        .filter(|w| w.len() >= 3 && !NON_TERMS.contains(&w.as_str()))
        .collect()
}

const MAX_TERMS_PER_CONCEPT: usize = 8;
const MAX_PAIRS_PER_CONCEPT: usize = 10;
const MAX_USAGES_LISTED: usize = 20;

pub struct TermDrift;

const TERM_INSTRUCTIONS: &str = "The state lists how two words are used in function names within one concept of a codebase. Decide whether both words name the same domain thing, so that one word could replace the other everywhere without changing meaning.";

struct Usage {
    symbol: String,
    file: PathBuf,
}

impl Task for TermDrift {
    fn id(&self) -> &'static str {
        "term_drift"
    }

    fn summary(&self) -> &'static str {
        "find two words naming the same thing within one concept (needs `concept`)"
    }

    fn depends_on(&self) -> &'static [&'static str] {
        &["concept"]
    }

    fn criteria_text(&self) -> String {
        TERM_INSTRUCTIONS.to_owned()
    }

    fn setup_hint(&self, ctx: &TaskContext<'_>) -> Option<String> {
        let has_concepts = ctx
            .prior
            .and_then(|p| p.get("concept"))
            .is_some_and(|m| !m.is_empty());
        (!has_concepts).then(|| {
            "needs `concept` verdicts first; run `heal semantic ask --task concept`".to_owned()
        })
    }

    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let Some(concepts) = load_concepts(ctx)? else {
            return Ok(Vec::new());
        };
        let descriptions: BTreeMap<String, String> = concepts.labels().into_iter().collect();
        // concept → term → usages
        let mut terms: BTreeMap<String, BTreeMap<String, Vec<Usage>>> = BTreeMap::new();
        // concept → symbols (as word sets), to reject pairs that co-occur
        let mut names: BTreeMap<String, Vec<BTreeSet<String>>> = BTreeMap::new();
        for group in ConceptTask.plan(ctx)? {
            for item in group.items {
                let Some((concept, p, _)) = ctx.prior_answer("concept", &item.key).and_then(chosen)
                else {
                    continue;
                };
                if p < 0.5 || concept == OTHER {
                    continue;
                }
                let symbol = item.meta["symbol"].as_str().unwrap_or_default().to_owned();
                let file = PathBuf::from(item.meta["file"].as_str().unwrap_or_default());
                let ws = term_words(&symbol);
                for w in &ws {
                    terms
                        .entry(concept.to_owned())
                        .or_default()
                        .entry(w.clone())
                        .or_default()
                        .push(Usage {
                            symbol: symbol.clone(),
                            file: file.clone(),
                        });
                }
                names.entry(concept.to_owned()).or_default().push(ws);
            }
        }
        let mut groups = Vec::new();
        for (concept, by_term) in &terms {
            let mut top: Vec<(&String, &Vec<Usage>)> =
                by_term.iter().filter(|(_, u)| u.len() >= 2).collect();
            top.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(b.0)));
            top.truncate(MAX_TERMS_PER_CONCEPT);
            let desc = descriptions.get(concept).cloned().unwrap_or_default();
            let mut items = Vec::new();
            for (i, (a, ua)) in top.iter().enumerate() {
                for (b, ub) in top.iter().skip(i + 1) {
                    let together = names[concept]
                        .iter()
                        .any(|ws| ws.contains(*a) && ws.contains(*b));
                    if together || items.len() >= MAX_PAIRS_PER_CONCEPT {
                        continue;
                    }
                    let list = |t: &str, u: &[Usage]| {
                        let shown: Vec<String> = u
                            .iter()
                            .take(MAX_USAGES_LISTED)
                            .map(|x| format!("  {} ({})", x.symbol, x.file.display()))
                            .collect();
                        format!("`{t}` appears in {} names:\n{}", u.len(), shown.join("\n"))
                    };
                    let subject = format!("{}\n\n{}", list(a, ua), list(b, ub));
                    let state = format!("Concept `{concept}`: {desc}");
                    items.push(Item {
                        key: ctx.key(self, &subject, &state),
                        question: Question::Noul {
                            instructions: json!(format!("{TERM_INSTRUCTIONS}\n\n{subject}")),
                            criteria: Some(NoulCriteria {
                                yes: json!(format!("`{a}` and `{b}` name the same thing in this concept.")),
                                no: json!(format!("`{a}` and `{b}` name different things, or one is a part or kind of the other.")),
                            }),
                        },
                        meta: json!({
                            "concept": concept,
                            "a": a, "a_count": ua.len(), "a_files": ua.iter().map(|u| u.file.to_string_lossy()).collect::<BTreeSet<_>>(),
                            "b": b, "b_count": ub.len(), "b_files": ub.iter().map(|u| u.file.to_string_lossy()).collect::<BTreeSet<_>>(),
                        }),
                    });
                }
            }
            if !items.is_empty() {
                groups.push(Group {
                    state: json!(format!("Concept `{concept}`: {desc}")),
                    items,
                });
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
            let (ca, cb) = (
                m["a_count"].as_u64().unwrap_or(0),
                m["b_count"].as_u64().unwrap_or(0),
            );
            let (major, minor, minor_files) = if ca >= cb {
                (&m["a"], &m["b"], &m["b_files"])
            } else {
                (&m["b"], &m["a"], &m["a_files"])
            };
            let (major, minor) = (major.as_str().unwrap_or(""), minor.as_str().unwrap_or(""));
            let files: Vec<PathBuf> = minor_files
                .as_array()
                .map(|v| {
                    v.iter()
                        .filter_map(Value::as_str)
                        .map(PathBuf::from)
                        .collect()
                })
                .unwrap_or_default();
            let Some(first) = files.first() else {
                continue;
            };
            let concept = m["concept"].as_str().unwrap_or("");
            let mut f = finding(
                "term_drift",
                first,
                None,
                None,
                format!(
                    "`{minor}` and `{major}` name the same thing in concept `{concept}` ({} vs {} names)",
                    ca.min(cb),
                    ca.max(cb)
                ),
                &format!("term_drift:{concept}:{minor}:{major}"),
                Severity::Medium,
            );
            f.fix_hint = Some(format!(
                "use one word: rename `{minor}` to `{major}` (or the glossary's term) in every name"
            ));
            f = f.with_locations(
                files
                    .iter()
                    .skip(1)
                    .take(10)
                    .map(|p| crate::core::finding::Location::file(p.clone()))
                    .collect(),
            );
            lowered.findings.push(f);
        }
        lowered
    }
}

pub struct NameMismatch;

const NAME_INSTRUCTIONS: &str = "The state is source code with line numbers. Judge whether the named function's name, and its doc comment if it has one, describe what its body actually does.";
const NAME_LEVELS: [&str; 4] = [
    "Not applicable: the function is trivial, generated, or has no meaningful name.",
    "The name and doc comment match what the body does.",
    "Arguable: the name is vague or covers only part of what the body does.",
    "The name or doc comment claims something the body does not do, or hides a significant effect such as I/O, mutation, or an error it throws.",
];
const MAX_NAME_SUBJECTS_PER_FILE: usize = 60;

impl Task for NameMismatch {
    fn id(&self) -> &'static str {
        "name_mismatch"
    }

    fn summary(&self) -> &'static str {
        "functions whose name or doc comment does not match the body (hotspot files first)"
    }

    fn criteria_text(&self) -> String {
        format!("{NAME_INSTRUCTIONS}\n{}", NAME_LEVELS.join("\n"))
    }

    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        // Files with a hotspot Code finding, plus functions with a CCN /
        // Cognitive finding anywhere.
        let mut hot_files: BTreeSet<&Path> = BTreeSet::new();
        let mut flagged: BTreeSet<(&Path, &str)> = BTreeSet::new();
        for f in ctx.findings {
            if Family::for_metric(&f.metric) != Family::Code {
                continue;
            }
            if f.hotspot {
                hot_files.insert(&f.location.file);
            }
            if matches!(f.metric.as_str(), "ccn" | "cognitive") && f.severity >= Severity::Medium {
                if let Some(s) = &f.location.symbol {
                    flagged.insert((&f.location.file, s.as_str()));
                }
            }
        }
        let candidates: BTreeSet<&Path> = hot_files
            .iter()
            .copied()
            .chain(flagged.iter().map(|(f, _)| *f))
            .collect();
        let prod: BTreeSet<&PathBuf> = code_files(ctx).0.iter().collect();
        let mut groups = Vec::new();
        for rel in candidates {
            if !prod.contains(&rel.to_path_buf()) {
                continue;
            }
            let Some(facts) = file_facts(ctx, rel) else {
                continue;
            };
            let functions: Vec<_> = facts
                .functions
                .iter()
                .filter(|f| !f.name.starts_with("<anonymous"))
                .filter(|f| hot_files.contains(rel) || flagged.contains(&(rel, f.name.as_str())))
                .take(MAX_NAME_SUBJECTS_PER_FILE)
                .collect();
            if functions.is_empty() {
                continue;
            }
            let spans: Vec<(u32, u32)> =
                functions.iter().map(|f| (f.start_row, f.end_row)).collect();
            for (state, covered) in file_states(rel, &facts.source, &spans) {
                let items = covered
                    .iter()
                    .map(|&i| {
                        let f = &functions[i];
                        let subject = format!("`{}` (lines {}–{})", f.name, f.start_row, f.end_row);
                        Item {
                            key: ctx.key(self, &subject, &state),
                            question: Question::Score {
                                instructions: json!(format!("{NAME_INSTRUCTIONS}\nFunction: {subject}")),
                                criteria: NAME_LEVELS.iter().map(|l| json!(l)).collect(),
                            },
                            meta: json!({"file": rel.to_string_lossy(), "symbol": f.name, "start": f.start_row}),
                        }
                    })
                    .collect();
                groups.push(Group {
                    state: json!(state),
                    items,
                });
            }
        }
        Ok(groups)
    }

    fn lower(&self, ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        let cutoff = ctx.cutoff(self, 0.5);
        let mut lowered = Lowered::default();
        for a in answered {
            // Level 0 is "not applicable", so read the probabilities, not
            // the mean `score` (see `applicable_share`).
            let Some((applies, contradicts)) = a
                .answer
                .and_then(|ans| applicable_share(ans, NAME_LEVELS.len(), 3))
            else {
                continue;
            };
            if applies < 0.5 || contradicts < cutoff {
                continue;
            }
            let m = &a.item.meta;
            let symbol = m["symbol"].as_str().unwrap_or("");
            let file = PathBuf::from(m["file"].as_str().unwrap_or(""));
            let line = m["start"].as_u64().and_then(|n| u32::try_from(n).ok());
            let mut f = finding(
                "name_mismatch",
                &file,
                line,
                Some(symbol),
                format!("`{symbol}`: the name or doc comment does not match what the body does"),
                &format!("name_mismatch:{symbol}"),
                Severity::Medium,
            );
            f.fix_hint = Some(
                "propose names, then compare them with `heal semantic ask --task name_choice --focus <candidates.json> --json`"
                    .to_owned(),
            );
            lowered.findings.push(f);
        }
        lowered
    }
}

pub struct NameChoice;

const CHOICE_INSTRUCTIONS: &str = "The state is source code with line numbers. Pick the name that best describes what the named function does, as a reader new to the codebase would expect it to be called.";

#[derive(Debug, Deserialize)]
struct Candidates {
    candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    file: PathBuf,
    symbol: String,
    names: Vec<String>,
}

impl Task for NameChoice {
    fn id(&self) -> &'static str {
        "name_choice"
    }

    fn summary(&self) -> &'static str {
        "on demand: pick the best of the agent's candidate names (`--focus candidates.json`)"
    }

    fn on_demand(&self) -> bool {
        true
    }

    fn criteria_text(&self) -> String {
        CHOICE_INSTRUCTIONS.to_owned()
    }

    fn setup_hint(&self, _ctx: &TaskContext<'_>) -> Option<String> {
        Some(r#"pass --focus with {"candidates":[{"file":"src/a.rs","symbol":"f","names":["g","h"]}]}"#.to_owned())
    }

    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let Some(raw) = ctx.focus else {
            return Ok(Vec::new());
        };
        let parsed: Candidates = serde_json::from_str(raw)
            .map_err(|e| anyhow::anyhow!("--focus for name_choice must be candidates JSON: {e}"))?;
        let mut groups = Vec::new();
        for c in parsed.candidates {
            let Some(facts) = file_facts(ctx, &c.file) else {
                continue;
            };
            let Some(func) = facts.functions.iter().find(|f| f.name == c.symbol) else {
                continue;
            };
            let mut options: BTreeMap<String, Value> = BTreeMap::new();
            options.insert(
                c.symbol.clone(),
                json!(format!("Keep the current name `{}`.", c.symbol)),
            );
            for n in &c.names {
                options.insert(n.clone(), json!(format!("Rename it to `{n}`.")));
            }
            if options.len() < 2 {
                continue;
            }
            let labels: Vec<(String, String)> = options
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                .collect();
            let spans = [(func.start_row, func.end_row)];
            for (state, _) in file_states(&c.file, &facts.source, &spans) {
                let subject = format!(
                    "`{}` (lines {}–{})",
                    func.name, func.start_row, func.end_row
                );
                let candidates_list: Vec<&str> = labels.iter().map(|(k, _)| k.as_str()).collect();
                groups.push(Group {
                    state: json!(state.clone()),
                    items: vec![Item {
                        key: ctx.key(self, &format!("{subject} {candidates_list:?}"), &state),
                        question: Question::Choice {
                            instructions: json!(format!(
                                "{CHOICE_INSTRUCTIONS}\nFunction: {subject}"
                            )),
                            criteria: criteria(&labels),
                        },
                        meta: json!({"file": c.file.to_string_lossy(), "symbol": c.symbol}),
                    }],
                });
            }
        }
        Ok(groups)
    }

    fn report(&self, _ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Option<Value> {
        let rows: Vec<Value> = answered
            .iter()
            .filter_map(|a| {
                let answer = a.answer?;
                let (best, p, conf) = chosen(answer)?;
                let current = a.item.meta["symbol"].as_str().unwrap_or("");
                let p_current = match answer {
                    crate::semantic::api::Answer::Choice { probabilities, .. } => {
                        probabilities.get(current).copied().unwrap_or(0.0)
                    }
                    _ => 0.0,
                };
                // Rename only when a candidate clearly beats the current
                // name; a close call keeps the name the team already knows.
                let rename = best != current && p - p_current >= 0.2 && conf >= 0.5;
                Some(json!({
                    "file": a.item.meta["file"],
                    "symbol": current,
                    "best": best,
                    "p": p,
                    "p_current": p_current,
                    "confidence": conf,
                    "rename": rename,
                }))
            })
            .collect();
        Some(json!({ "choices": rows }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::api::Answer;

    #[test]
    fn name_mismatch_needs_contradiction_among_applicable_readings() {
        let cfg = crate::core::config::Config::default();
        let ctx = TaskContext::new(Path::new("."), &cfg).unwrap();
        let item = |symbol: &str| Item {
            key: symbol.to_owned(),
            question: Question::Noul {
                instructions: json!(""),
                criteria: None,
            },
            meta: json!({"file": "src/a.rs", "symbol": symbol, "start": 1}),
        };
        let answer = |probs: [f64; 4]| Answer::Score {
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
        };
        let items = [item("hides_io"), item("vague"), item("trivial")];
        let answers = [
            // A third "n/a" pulls the mean to 1.86, yet among applicable
            // readings the name contradicts the body.
            answer([0.33, 0.02, 0.05, 0.60]),
            // "Arguable" is not a mismatch.
            answer([0.01, 0.16, 0.53, 0.30]),
            // Not applicable.
            answer([0.70, 0.00, 0.00, 0.30]),
        ];
        let answered: Vec<Answered<'_>> = items
            .iter()
            .zip(&answers)
            .map(|(item, a)| Answered {
                item,
                answer: Some(a),
            })
            .collect();
        let lowered = NameMismatch.lower(&ctx, &answered);
        let symbols: Vec<&str> = lowered
            .findings
            .iter()
            .filter_map(|f| f.location.symbol.as_deref())
            .collect();
        assert_eq!(symbols, ["hides_io"]);
    }

    #[test]
    fn plural_forms_are_one_term() {
        for (plural, one) in [
            ("functions", "function"),
            ("clusters", "cluster"),
            ("counts", "count"),
            ("models", "model"),
            ("queries", "query"),
            ("classes", "class"),
            ("indexes", "index"),
            ("matches", "match"),
        ] {
            assert_eq!(singular(plural), one, "{plural}");
        }
        for kept in ["class", "status", "analysis", "alias", "bus", "is"] {
            assert_eq!(singular(kept), kept);
        }
        assert_eq!(
            term_words("extract_functions"),
            term_words("extract_function")
        );
        assert!(term_words("has_multiple_clusters").contains("cluster"));
        assert!(!term_words("get_models").contains("get"));
    }

    #[test]
    fn words_split_every_case_style() {
        assert_eq!(words("loadUserAccount"), ["load", "user", "account"]);
        assert_eq!(words("load_user_account"), ["load", "user", "account"]);
        assert_eq!(words("HTTPServer"), ["httpserver"]);
        assert_eq!(words("UserId2"), ["user", "id2"]);
        assert_eq!(words("kebab-case"), ["kebab", "case"]);
    }
}
