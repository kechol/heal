//! `concept`: classify every production function into one concept of
//! `.heal/concepts.toml`, then read the resulting concept map for three
//! structural problems a metric cannot see:
//!
//! - `concept_mix` — one file carries two or more substantial concepts
//!   → split it along concept lines.
//! - `concept_misplaced` — a function's concept is not its file's main
//!   concept, and another file is mainly about that concept → move it.
//! - `concept_scatter` — one concept is spread thinly over many files
//!   with no home → consolidate it.
//!
//! The approach follows conceptual cohesion / coupling (Marcus &
//! Poshyvanyk, "The Conceptual Cohesion of Classes", ICSM 2005), which
//! measured meaning from identifiers and comments with LSI; here the
//! meaning comes from a typed classification against a vocabulary the
//! team wrote, so every finding can name the concept involved.
//!
//! State is the whole file (numbered); one `choice` per function. Any
//! edit to a file re-asks that file's functions and nothing else.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::core::concepts::{Concepts, OTHER};
use crate::core::finding::Finding;
use crate::core::severity::Severity;
use crate::semantic::task::{Answered, Group, Item, Lowered, Task, TaskContext};
use crate::semantic::tasks::common::{
    chosen, code_files, criteria, file_facts, file_states, finding,
};

pub struct ConceptTask;

/// Functions shorter than this are not worth a question.
const MIN_FUNCTION_LINES: u32 = 3;
/// A concept is "substantial" in a file at this share of classified LOC.
const MIX_SHARE: f64 = 0.25;
/// Files smaller than this (classified LOC) are never `concept_mix`.
const MIX_MIN_LOC: u32 = 60;
/// A file is "about" a concept at this share.
const HOME_SHARE: f64 = 0.5;
/// `concept_scatter`: at least this many files, none holding this share.
const SCATTER_MIN_FILES: usize = 5;
const SCATTER_MAX_SHARE: f64 = 0.4;

const INSTRUCTIONS: &str = "The state is one source file with line numbers. Decide which concept the named function mainly implements: what it is responsible for, not which words it uses.";

pub(crate) fn load_concepts(ctx: &TaskContext<'_>) -> anyhow::Result<Option<Concepts>> {
    Ok(Concepts::load(
        &crate::core::HealPaths::new(ctx.project).concepts(),
    )?)
}

impl Task for ConceptTask {
    fn id(&self) -> &'static str {
        "concept"
    }

    fn summary(&self) -> &'static str {
        "map every function to a concept; flag mixed files, misplaced functions, scattered concepts"
    }

    fn criteria_text(&self) -> String {
        INSTRUCTIONS.to_owned()
    }

    fn setup_hint(&self, ctx: &TaskContext<'_>) -> Option<String> {
        (!crate::core::HealPaths::new(ctx.project).concepts().exists()).then(|| {
            "no .heal/concepts.toml yet; run /heal-concepts-setup to write the vocabulary"
                .to_owned()
        })
    }

    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let Some(concepts) = load_concepts(ctx)? else {
            return Ok(Vec::new());
        };
        let labels = concepts.labels();
        let vocab = labels.iter().fold(String::new(), |mut acc, (k, v)| {
            let _ = writeln!(acc, "{k}: {v}");
            acc
        });
        let mut groups = Vec::new();
        for rel in &code_files(ctx).0 {
            let Some(facts) = file_facts(ctx, rel) else {
                continue;
            };
            // Inline unit tests (and their helpers) are not production
            // code: classifying them skews the concept map and suggests
            // moving tests next to whatever concept their name mentions.
            let functions: Vec<_> = facts
                .production_functions()
                .filter(|f| f.end_row - f.start_row + 1 >= MIN_FUNCTION_LINES)
                .collect();
            if functions.is_empty() {
                continue;
            }
            let spans: Vec<(u32, u32)> =
                functions.iter().map(|f| (f.start_row, f.end_row)).collect();
            for (state, covered) in file_states(rel, &facts.source, &spans) {
                let state_key = format!("{state}\n--\n{vocab}");
                let items = covered
                    .iter()
                    .map(|&i| {
                        let f = &functions[i];
                        let subject = format!("`{}` (lines {}–{})", f.name, f.start_row, f.end_row);
                        Item {
                            key: ctx.key(self, &subject, &state_key),
                            question: crate::semantic::api::Question::Choice {
                                instructions: json!(format!("{INSTRUCTIONS}\nFunction: {subject}")),
                                criteria: criteria(&labels),
                            },
                            meta: json!({
                                "file": rel.to_string_lossy(),
                                "symbol": f.name,
                                "start": f.start_row,
                                "end": f.end_row,
                            }),
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
        let map = ConceptMap::build(answered, cutoff);
        let mut lowered = Lowered::default();
        lowered.findings.extend(map.mixed());
        lowered.findings.extend(map.misplaced());
        lowered.findings.extend(map.scattered());
        lowered
    }
}

/// One classified function.
#[derive(Debug, Clone)]
pub(crate) struct Placed {
    pub file: PathBuf,
    pub symbol: String,
    pub start: u32,
    pub loc: u32,
    pub concept: String,
}

/// Classified LOC per file per concept, from confident answers only.
#[derive(Debug, Default)]
pub(crate) struct ConceptMap {
    pub placed: Vec<Placed>,
    /// file → concept → LOC
    pub by_file: BTreeMap<PathBuf, BTreeMap<String, u32>>,
}

fn meta_str(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_owned()
}

fn meta_u32(v: &Value, k: &str) -> u32 {
    v[k].as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0)
}

impl ConceptMap {
    pub(crate) fn build(answered: &[Answered<'_>], cutoff: f64) -> Self {
        let mut map = Self::default();
        for a in answered {
            let Some((label, p, _)) = a.answer.and_then(chosen) else {
                continue;
            };
            if p < cutoff || label == OTHER {
                continue;
            }
            let m = &a.item.meta;
            let (start, end) = (meta_u32(m, "start"), meta_u32(m, "end"));
            let placed = Placed {
                file: PathBuf::from(meta_str(m, "file")),
                symbol: meta_str(m, "symbol"),
                start,
                loc: end.saturating_sub(start) + 1,
                concept: label.to_owned(),
            };
            *map.by_file
                .entry(placed.file.clone())
                .or_default()
                .entry(placed.concept.clone())
                .or_insert(0) += placed.loc;
            map.placed.push(placed);
        }
        map
    }

    fn shares(concepts: &BTreeMap<String, u32>) -> Vec<(String, f64)> {
        let total: u32 = concepts.values().sum();
        if total == 0 {
            return Vec::new();
        }
        let mut v: Vec<(String, f64)> = concepts
            .iter()
            .map(|(c, loc)| (c.clone(), f64::from(*loc) / f64::from(total)))
            .collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    }

    /// The concept a file is mainly about, if one holds `HOME_SHARE`.
    pub(crate) fn home_of(&self, file: &PathBuf) -> Option<String> {
        let s = Self::shares(self.by_file.get(file)?);
        s.first()
            .filter(|(_, sh)| *sh >= HOME_SHARE)
            .map(|(c, _)| c.clone())
    }

    fn mixed(&self) -> Vec<Finding> {
        let mut out = Vec::new();
        for (file, concepts) in &self.by_file {
            let total: u32 = concepts.values().sum();
            if total < MIX_MIN_LOC {
                continue;
            }
            let big: Vec<(String, f64)> = Self::shares(concepts)
                .into_iter()
                .filter(|(_, s)| *s >= MIX_SHARE)
                .collect();
            if big.len() < 2 {
                continue;
            }
            let ids: Vec<&str> = big.iter().map(|(c, _)| c.as_str()).collect();
            let desc: Vec<String> = big
                .iter()
                .map(|(c, s)| format!("{c} ({:.0}%)", s * 100.0))
                .collect();
            let severity = if big.len() >= 3 {
                Severity::High
            } else {
                Severity::Medium
            };
            let mut f = finding(
                "concept_mix",
                file,
                None,
                None,
                format!("mixes {} concepts: {}", big.len(), desc.join(", ")),
                &format!("concept_mix:{}", ids.join("+")),
                severity,
            );
            f.fix_hint = Some(format!(
                "split along concept lines; functions per concept: {}",
                big.iter()
                    .map(|(c, _)| {
                        let names: Vec<&str> = self
                            .placed
                            .iter()
                            .filter(|p| &p.file == file && &p.concept == c)
                            .map(|p| p.symbol.as_str())
                            .collect();
                        format!("{c} = [{}]", names.join(", "))
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
            out.push(f);
        }
        out
    }

    fn misplaced(&self) -> Vec<Finding> {
        let homes: BTreeMap<String, Vec<&PathBuf>> =
            self.by_file.keys().fold(BTreeMap::new(), |mut acc, f| {
                if let Some(c) = self.home_of(f) {
                    acc.entry(c).or_insert_with(Vec::new).push(f);
                }
                acc
            });
        let mut out = Vec::new();
        for p in &self.placed {
            let Some(home) = self.home_of(&p.file) else {
                continue;
            };
            if home == p.concept {
                continue;
            }
            let Some(target) = homes
                .get(&p.concept)
                .and_then(|v| v.iter().find(|f| **f != &p.file))
            else {
                continue;
            };
            let mut f = finding(
                "concept_misplaced",
                &p.file,
                Some(p.start),
                Some(&p.symbol),
                format!(
                    "`{}` implements `{}`, but this file is about `{home}`; `{}` is where `{}` lives",
                    p.symbol,
                    p.concept,
                    target.display(),
                    p.concept
                ),
                &format!("concept_misplaced:{}:{}", p.symbol, p.concept),
                Severity::Medium,
            );
            f.fix_hint = Some(format!("move `{}` to `{}`", p.symbol, target.display()));
            out.push(f);
        }
        out
    }

    fn scattered(&self) -> Vec<Finding> {
        let mut per_concept: BTreeMap<&str, Vec<(&PathBuf, u32)>> = BTreeMap::new();
        for (file, concepts) in &self.by_file {
            for (c, loc) in concepts {
                per_concept
                    .entry(c.as_str())
                    .or_default()
                    .push((file, *loc));
            }
        }
        let mut out = Vec::new();
        for (concept, mut files) in per_concept {
            if files.len() < SCATTER_MIN_FILES {
                continue;
            }
            let total: u32 = files.iter().map(|(_, l)| l).sum();
            files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
            let top_share = f64::from(files[0].1) / f64::from(total.max(1));
            if top_share >= SCATTER_MAX_SHARE {
                continue;
            }
            let others = files
                .iter()
                .skip(1)
                .take(10)
                .map(|(f, _)| crate::core::finding::Location::file((*f).clone()))
                .collect();
            out.push(
                finding(
                    "concept_scatter",
                    files[0].0,
                    None,
                    None,
                    format!(
                        "concept `{concept}` is spread over {} files; the largest holds {:.0}%",
                        files.len(),
                        top_share * 100.0
                    ),
                    &format!("concept_scatter:{concept}"),
                    Severity::Medium,
                )
                .with_locations(others),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::api::Answer;
    use crate::semantic::task::Item;

    fn item(file: &str, symbol: &str, start: u32, end: u32) -> Item {
        Item {
            key: format!("{file}:{symbol}"),
            question: crate::semantic::api::Question::Noul {
                instructions: json!(""),
                criteria: None,
            },
            meta: json!({"file": file, "symbol": symbol, "start": start, "end": end}),
        }
    }

    fn choice(c: &str) -> Answer {
        crate::semantic::tasks::testing::choice(c, 0.9)
    }

    #[test]
    fn detects_mix_misplaced_and_scatter() {
        let mut items = vec![
            // a.rs: parsing 50 + storage 40 lines → mixed
            item("a.rs", "parse", 1, 50),
            item("a.rs", "save", 51, 90),
            // store.rs: storage home
            item("store.rs", "load", 1, 80),
            // b.rs: parsing home, with one storage fn → misplaced (store.rs is home)
            item("b.rs", "lex", 1, 90),
            item("b.rs", "persist", 91, 100),
        ];
        let mut answers = vec![
            choice("parsing"),
            choice("storage"),
            choice("storage"),
            choice("parsing"),
            choice("storage"),
        ];
        // logging: 6 files × 10 lines, no home → scatter
        for i in 0..6 {
            items.push(item(&format!("l{i}.rs"), "log", 1, 10));
            items.push(item(&format!("l{i}.rs"), "main", 11, 40));
            answers.push(choice("logging"));
            answers.push(choice(&format!("app{i}")));
        }
        let answered: Vec<Answered<'_>> = items
            .iter()
            .zip(&answers)
            .map(|(item, a)| Answered {
                item,
                answer: Some(a),
            })
            .collect();
        let map = ConceptMap::build(&answered, 0.5);
        let mixed = map.mixed();
        assert_eq!(mixed.len(), 1);
        assert_eq!(mixed[0].location.file, PathBuf::from("a.rs"));
        assert!(
            mixed[0].summary.contains("parsing (56%)"),
            "{}",
            mixed[0].summary
        );

        let misplaced = map.misplaced();
        let hit: Vec<_> = misplaced
            .iter()
            .map(|f| f.location.symbol.clone().unwrap())
            .collect();
        assert!(hit.contains(&"persist".to_owned()), "{hit:?}");
        assert!(misplaced.iter().all(|f| f.severity == Severity::Medium));

        let scatter = map.scattered();
        assert_eq!(scatter.len(), 1);
        assert!(scatter[0].summary.contains("`logging`"));
        assert_eq!(scatter[0].locations.len(), 5);
    }

    #[test]
    fn low_confidence_and_other_are_ignored() {
        let items = [item("a.rs", "f", 1, 100)];
        let low = crate::semantic::tasks::testing::choice("x", 0.3);
        let other = choice(OTHER);
        for a in [&low, &other] {
            let answered = [Answered {
                item: &items[0],
                answer: Some(a),
            }];
            assert!(ConceptMap::build(&answered, 0.5).placed.is_empty());
        }
    }
}
