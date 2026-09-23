//! Test-family tasks.
//!
//! Good tests protect against regressions, survive refactoring, run
//! fast, and are cheap to maintain (the four pillars in Khorikov, *Unit
//! Testing Principles, Practices, and Patterns*, 2020). Speed HEAL can
//! measure elsewhere; these tasks ask about the other three, one
//! question per pillar, so a test that exists only to raise coverage —
//! common in agent-written suites — can be found and removed.
//!
//! - `test_value` (T7) — per test case: does it fail when the behaviour
//!   its name claims breaks; does it fail after a behaviour-preserving
//!   rewrite; what does it actually check; does the code under test
//!   contain logic that can be wrong. → `test_value` findings labelled
//!   `delete` or `rewrite`.
//! - `mock_scope` (T8) — per mock set-up: what it replaces. Mocks belong
//!   at out-of-process boundaries; mocking an internal collaborator ties
//!   the test to implementation details. → `mock_scope` findings.
//! - `test_triage` (H7) — the test-review skill's classifications:
//!   uncovered source band, skip reason.
//! - `verify_tests` (V2, on demand) — the `test_value` / `mock_scope`
//!   questions for tests added in `--diff`, so a patch session can stop a
//!   worthless test before it lands.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::core::finding::SemanticNote;
use crate::core::severity::Severity;
use crate::observer::test::cases::{mock_sites, test_cases, TestCase};
use crate::semantic::api::{Answer, NoulCriteria, Question};
use crate::semantic::task::{Answered, Group, Item, Lowered, Task, TaskContext};
use crate::semantic::tasks::common::{
    chosen, code_files, criteria, file_states, finding, guess_src_for_test, note, noul_p, numbered,
    parse_file, TestMatcher,
};

// ------------------------------------------------------------- T7

pub const CHECKS: [(&str, &str); 5] = [
    (
        "behaviour",
        "The behaviour of the code under test: its outputs, effects, or errors for given inputs.",
    ),
    (
        "mock_values",
        "Only values or calls the test itself configured on mocks or stubs (a tautology).",
    ),
    (
        "framework",
        "The language, a library, or the test framework rather than the project's code.",
    ),
    (
        "constants_or_types",
        "A constant, a type, a default value, or a plain field assignment.",
    ),
    (
        "nothing",
        "Nothing meaningful: no assertion, or an assertion that cannot fail.",
    ),
];

const VALUE_INSTRUCTIONS: &str = "The state is a test file with line numbers, and the code under test when it could be found. Answer about the named test case.";

fn value_questions(subject: &str) -> [(&'static str, Question); 4] {
    [
        (
            "regression",
            Question::Noul {
                instructions: json!(format!("{VALUE_INSTRUCTIONS}\nTest: {subject}\nIf the behaviour this test's name describes were broken, this test would fail.")),
                criteria: Some(NoulCriteria {
                    yes: json!("It would fail."),
                    no: json!("It would still pass."),
                }),
            },
        ),
        (
            "brittle",
            Question::Noul {
                instructions: json!(format!("{VALUE_INSTRUCTIONS}\nTest: {subject}\nThis test would fail after an internal rewrite that keeps behaviour the same, because it checks implementation details (private calls, call counts on internal collaborators, internal data layout).")),
                criteria: Some(NoulCriteria {
                    yes: json!("It is tied to implementation details."),
                    no: json!("It only depends on observable behaviour."),
                }),
            },
        ),
        (
            "checks",
            Question::Choice {
                instructions: json!(format!("{VALUE_INSTRUCTIONS}\nTest: {subject}\nWhat does this test actually check?")),
                criteria: criteria(&CHECKS.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect::<Vec<_>>()),
            },
        ),
        (
            "has_logic",
            Question::Noul {
                instructions: json!(format!("{VALUE_INSTRUCTIONS}\nTest: {subject}\nThe code this test exercises contains logic that can be wrong, not only a getter, a constructor, or a pass-through.")),
                criteria: None,
            },
        ),
    ]
}

/// The state for questions about `cases` of `test_file`: the numbered
/// test file (or windows), plus the guessed source file when both fit.
fn test_states(
    ctx: &TaskContext<'_>,
    test_file: &Path,
    source: &str,
    spans: &[(u32, u32)],
) -> Vec<(String, Vec<usize>)> {
    let states = file_states(test_file, source, spans);
    let Some(src_path) = guess_src_for_test(ctx, test_file) else {
        return states;
    };
    let Some(src) = ctx.read_sendable(&src_path) else {
        return states;
    };
    let budget = crate::semantic::cost::state_budget();
    states
        .into_iter()
        .map(|(state, covered)| {
            let with = format!(
                "{state}\n\nCode under test, {}:\n{}",
                src_path.display(),
                numbered(&src)
            );
            if crate::semantic::cost::estimate_tokens(with.len()) <= budget {
                (with, covered)
            } else {
                (state, covered)
            }
        })
        .collect()
}

fn value_groups(
    task: &dyn Task,
    ctx: &TaskContext<'_>,
    file: &Path,
    source: &str,
    cases: &[TestCase],
) -> Vec<Group> {
    let spans: Vec<(u32, u32)> = cases.iter().map(|c| (c.start_line, c.end_line)).collect();
    test_states(ctx, file, source, &spans)
        .into_iter()
        .map(|(state, covered)| {
            let items = covered
                .iter()
                .flat_map(|&i| {
                    let c = &cases[i];
                    let subject = format!("`{}` (lines {}–{})", c.name, c.start_line, c.end_line);
                    value_questions(&subject)
                        .into_iter()
                        .map(move |(q, question)| (q, question, subject.clone(), c))
                })
                .map(|(q, question, subject, c)| Item {
                    key: ctx.key(task, &format!("{q}:{subject}"), &state),
                    question,
                    meta: json!({
                        "file": file.to_string_lossy(),
                        "test": c.name,
                        "start": c.start_line,
                        "end": c.end_line,
                        "q": q,
                    }),
                })
                .collect();
            Group {
                state: json!(state),
                items,
            }
        })
        .collect()
}

/// One test case's four answers.
#[derive(Debug, Default, Clone)]
pub(crate) struct Verdict4 {
    pub regression: Option<f64>,
    pub brittle: Option<f64>,
    pub checks: Option<(String, f64, f64)>,
    pub has_logic: Option<f64>,
}

impl Verdict4 {
    /// `delete`, `rewrite`, or `None` (keep / not enough answers).
    pub(crate) fn label(&self) -> Option<&'static str> {
        let regression = self.regression?;
        let (checks, _, conf) = self.checks.as_ref()?;
        let trivial = matches!(
            checks.as_str(),
            "mock_values" | "framework" | "nothing" | "constants_or_types"
        );
        if trivial && *conf >= 0.9 && regression < 0.5 {
            return Some("delete");
        }
        if self.has_logic.is_some_and(|p| p < 0.2) && regression < 0.5 {
            return Some("delete");
        }
        if regression >= 0.5 && self.brittle.is_some_and(|p| p >= 0.7) {
            return Some("rewrite");
        }
        None
    }
}

fn collect4(answered: &[Answered<'_>]) -> BTreeMap<(String, String, u32), Verdict4> {
    let mut out: BTreeMap<(String, String, u32), Verdict4> = BTreeMap::new();
    for a in answered {
        let Some(answer) = a.answer else { continue };
        let m = &a.item.meta;
        let key = (
            m["file"].as_str().unwrap_or("").to_owned(),
            m["test"].as_str().unwrap_or("").to_owned(),
            u32::try_from(m["start"].as_u64().unwrap_or(0)).unwrap_or(0),
        );
        let v = out.entry(key).or_default();
        match m["q"].as_str().unwrap_or("") {
            "regression" => v.regression = noul_p(answer),
            "brittle" => v.brittle = noul_p(answer),
            "has_logic" => v.has_logic = noul_p(answer),
            "checks" => v.checks = chosen(answer).map(|(l, p, c)| (l.to_owned(), p, c)),
            _ => {}
        }
    }
    out
}

pub struct TestValue;

impl Task for TestValue {
    fn id(&self) -> &'static str {
        "test_value"
    }
    fn summary(&self) -> &'static str {
        "tests that check nothing, only their mocks, or implementation details (delete / rewrite candidates)"
    }
    fn criteria_text(&self) -> String {
        let mut s = VALUE_INSTRUCTIONS.to_owned();
        for (_, q) in value_questions("") {
            s.push('\n');
            s.push_str(&serde_json::to_string(&q).unwrap_or_default());
        }
        s
    }
    fn setup_hint(&self, ctx: &TaskContext<'_>) -> Option<String> {
        (!ctx.config.features.test.enabled)
            .then(|| "needs [features.test] enabled = true".to_owned())
    }
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        if !ctx.config.features.test.enabled {
            return Ok(Vec::new());
        }
        let mut groups = Vec::new();
        for file in code_files(ctx).1 {
            let Some(parsed) = parse_file(ctx, &file) else {
                continue;
            };
            let cases: Vec<TestCase> = test_cases(&parsed)
                .into_iter()
                .filter(|c| !c.skipped)
                .collect();
            if cases.is_empty() {
                continue;
            }
            groups.extend(value_groups(self, ctx, &file, &parsed.source, &cases));
        }
        Ok(groups)
    }
    fn lower(&self, _ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        let mut lowered = Lowered::default();
        for ((file, test, start), v) in collect4(answered) {
            let Some(label) = v.label() else { continue };
            let checks = v.checks.as_ref().map_or("?", |c| c.0.as_str());
            let (summary, hint) = if label == "delete" {
                (
                    format!("`{test}` checks {checks} and would not catch the behaviour its name claims breaking"),
                    "delete it (one test per commit; see heal-test-patch for the coverage guard) or rewrite it to assert behaviour",
                )
            } else {
                (
                    format!("`{test}` would break on a behaviour-preserving refactor (tied to implementation details)"),
                    "rewrite it to assert observable behaviour instead of internal calls or layout",
                )
            };
            let mut f = finding(
                "test_value",
                Path::new(&file),
                Some(start),
                Some(&test),
                summary,
                &format!("test_value:{test}"),
                Severity::Medium,
            );
            f.fix_hint = Some(hint.to_owned());
            f.semantic.insert(
                "test_value".to_owned(),
                SemanticNote {
                    label: label.to_owned(),
                    p: v.regression.unwrap_or(0.0),
                    confidence: v.checks.as_ref().map_or(0.0, |c| c.2),
                    lines: Vec::new(),
                    detail: Some(format!(
                        "regression={:.2} brittle={:.2} checks={checks} has_logic={:.2}",
                        v.regression.unwrap_or(f64::NAN),
                        v.brittle.unwrap_or(f64::NAN),
                        v.has_logic.unwrap_or(f64::NAN)
                    )),
                },
            );
            lowered.findings.push(f);
        }
        lowered
    }
}

// ------------------------------------------------------------- T8

pub const MOCK_KINDS: [(&str, &str); 4] = [
    ("boundary", "An out-of-process dependency the test cannot control: network, database, file system, clock, randomness, or a third-party service."),
    ("subject", "The very code the test is supposed to test."),
    ("internal_collaborator", "Another part of the same codebase that could run for real in the test."),
    ("pure_value", "A pure function, value object, or data that needs no substitute."),
];
const MOCK_INSTRUCTIONS: &str = "The state is a test file with line numbers. Decide what the mock, stub, or spy set up on the named line replaces.";

fn mock_items(
    task: &dyn Task,
    ctx: &TaskContext<'_>,
    file: &Path,
    state: &str,
    lines: &[(u32, String)],
) -> Vec<Item> {
    let labels: Vec<(String, String)> = MOCK_KINDS
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    lines
        .iter()
        .map(|(line, text)| Item {
            key: ctx.key(task, text, state),
            question: Question::Choice {
                instructions: json!(format!("{MOCK_INSTRUCTIONS}\nLine {line}: {text}")),
                criteria: criteria(&labels),
            },
            meta: json!({"file": file.to_string_lossy(), "line": line, "text": text}),
        })
        .collect()
}

pub struct MockScope;

impl Task for MockScope {
    fn id(&self) -> &'static str {
        "mock_scope"
    }
    fn summary(&self) -> &'static str {
        "mocks that replace the subject, an internal collaborator, or a pure value"
    }
    fn criteria_text(&self) -> String {
        let mut s = MOCK_INSTRUCTIONS.to_owned();
        for (k, v) in MOCK_KINDS {
            s.push_str(&format!("\n{k}: {v}"));
        }
        s
    }
    fn setup_hint(&self, ctx: &TaskContext<'_>) -> Option<String> {
        (!ctx.config.features.test.enabled)
            .then(|| "needs [features.test] enabled = true".to_owned())
    }
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        if !ctx.config.features.test.enabled {
            return Ok(Vec::new());
        }
        let mut groups = Vec::new();
        for file in code_files(ctx).1 {
            let Some(parsed) = parse_file(ctx, &file) else {
                continue;
            };
            let sites = mock_sites(&parsed.source, parsed.lang);
            if sites.is_empty() {
                continue;
            }
            let spans: Vec<(u32, u32)> = sites.iter().map(|s| (s.line, s.line)).collect();
            for (state, covered) in test_states(ctx, &file, &parsed.source, &spans) {
                let lines: Vec<(u32, String)> = covered
                    .iter()
                    .map(|&i| (sites[i].line, sites[i].text.clone()))
                    .collect();
                groups.push(Group {
                    items: mock_items(self, ctx, &file, &state, &lines),
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
            let Some((kind, p, _)) = chosen(answer) else {
                continue;
            };
            if p < cutoff || kind == "boundary" {
                continue;
            }
            let m = &a.item.meta;
            let file = PathBuf::from(m["file"].as_str().unwrap_or(""));
            let line = u32::try_from(m["line"].as_u64().unwrap_or(0)).unwrap_or(0);
            let text = m["text"].as_str().unwrap_or("");
            let (severity, summary, hint) = match kind {
                "subject" => (Severity::High, "the mock replaces the code under test, so the test proves nothing about it", "test the real code; remove the mock"),
                "internal_collaborator" => (Severity::Medium, "the mock replaces an internal collaborator, tying the test to implementation details", "use the real collaborator; keep mocks at process boundaries"),
                _ => (Severity::Medium, "the mock replaces a pure value or function that needs no substitute", "use the real value"),
            };
            let mut f = finding(
                "mock_scope",
                &file,
                Some(line),
                None,
                format!("{summary}: `{text}`"),
                &format!(
                    "mock_scope:{}",
                    crate::semantic::store::content_hash(text.as_bytes())
                ),
                severity,
            );
            f.fix_hint = Some(hint.to_owned());
            f.semantic
                .insert("mock_scope".to_owned(), note(kind, answer, 0));
            lowered.findings.push(f);
        }
        lowered
    }
}

// ------------------------------------------------------------- H7

pub struct TestTriage;

const BANDS: [(&str, &str); 3] = [
    ("pure_logic", "Pure logic: computations and decisions that a unit test can check directly."),
    ("coordination", "Coordination or orchestration: code that mostly calls other components in order."),
    ("io_boundary", "An I/O boundary: code that talks to files, network, databases, processes, or the terminal."),
];
const SKIP_REASONS: [(&str, &str); 4] = [
    (
        "environment",
        "Skipped because it needs a specific platform, hardware, service, or CI environment.",
    ),
    ("slow", "Skipped because it is slow."),
    ("broken", "Skipped because it fails or is flaky."),
    (
        "pending",
        "Skipped because the feature is not implemented yet (test-first).",
    ),
];

impl Task for TestTriage {
    fn id(&self) -> &'static str {
        "test_triage"
    }
    fn summary(&self) -> &'static str {
        "classify uncovered source (pure logic / coordination / I/O boundary) and skipped tests' reasons"
    }
    fn criteria_text(&self) -> String {
        let mut s = String::new();
        for (k, v) in BANDS.iter().chain(SKIP_REASONS.iter()) {
            s.push_str(&format!("{k}: {v}\n"));
        }
        s
    }
    fn setup_hint(&self, ctx: &TaskContext<'_>) -> Option<String> {
        (!ctx.config.features.test.enabled)
            .then(|| "needs [features.test] enabled = true".to_owned())
    }
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        if !ctx.config.features.test.enabled {
            return Ok(Vec::new());
        }
        let to_labels = |set: &[(&str, &str)]| -> Vec<(String, String)> {
            set.iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect()
        };
        let mut groups = Vec::new();
        for f in ctx.findings {
            if f.accepted || f.severity == Severity::Ok {
                continue;
            }
            match f.metric.as_str() {
                "coverage_pct" => {
                    let Some(src) = ctx.read_sendable(&f.location.file) else {
                        continue;
                    };
                    let state = format!(
                        "File: {}\n\n{}",
                        f.location.file.display(),
                        crate::semantic::tasks::common::numbered_range(&src, 1, 300)
                    );
                    groups.push(Group {
                        items: vec![Item {
                            key: ctx.key(self, "band", &state),
                            question: Question::Choice {
                                instructions: json!("The state is a source file with too little test coverage. What kind of code is it mostly?"),
                                criteria: criteria(&to_labels(&BANDS)),
                            },
                            meta: json!({"id": f.id, "note": "coverage_band"}),
                        }],
                        state: json!(state),
                    });
                }
                "skip_ratio" => {
                    let Some(parsed) = parse_file(ctx, &f.location.file) else {
                        continue;
                    };
                    let skipped: Vec<TestCase> = test_cases(&parsed)
                        .into_iter()
                        .filter(|c| c.skipped)
                        .collect();
                    if skipped.is_empty() {
                        continue;
                    }
                    let spans: Vec<(u32, u32)> =
                        skipped.iter().map(|c| (c.start_line, c.end_line)).collect();
                    for (state, covered) in file_states(&f.location.file, &parsed.source, &spans) {
                        let items = covered
                            .iter()
                            .map(|&i| {
                                let c = &skipped[i];
                                let subject = format!("`{}` (lines {}–{})", c.name, c.start_line, c.end_line);
                                Item {
                                    key: ctx.key(self, &subject, &state),
                                    question: Question::Choice {
                                        instructions: json!(format!("The state is a test file. Why is the skipped test {subject} skipped?")),
                                        criteria: criteria(&to_labels(&SKIP_REASONS)),
                                    },
                                    meta: json!({"id": f.id, "note": "skip_reason", "test": c.name}),
                                }
                            })
                            .collect();
                        groups.push(Group {
                            items,
                            state: json!(state),
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(groups)
    }
    fn lower(&self, _ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Lowered {
        let mut lowered = Lowered::default();
        // skip_ratio: per finding, the reasons of each skipped test.
        let mut skips: BTreeMap<String, Vec<(String, String, &Answer)>> = BTreeMap::new();
        for a in answered {
            let Some(answer) = a.answer else { continue };
            let Some((label, _, _)) = chosen(answer) else {
                continue;
            };
            let m = &a.item.meta;
            let id = m["id"].as_str().unwrap_or("").to_owned();
            if m["note"] == "skip_reason" {
                skips.entry(id).or_default().push((
                    m["test"].as_str().unwrap_or("").to_owned(),
                    label.to_owned(),
                    answer,
                ));
            } else {
                lowered
                    .notes
                    .push((id, "coverage_band".to_owned(), note(label, answer, 0)));
            }
        }
        for (id, list) in skips {
            let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
            for (_, l, _) in &list {
                *counts.entry(l.as_str()).or_default() += 1;
            }
            let top = counts
                .iter()
                .max_by_key(|(_, n)| **n)
                .map_or("", |(l, _)| *l)
                .to_owned();
            let mut n = note(top, list[0].2, 0);
            n.detail = Some(
                list.iter()
                    .map(|(t, l, _)| format!("{t}: {l}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            lowered.notes.push((id, "skip_reason".to_owned(), n));
        }
        lowered
    }
}

// ------------------------------------------------------------- V2

pub struct VerifyTests;

/// New-side line ranges added in a unified patch (`@@ -a,b +c,d @@`).
fn added_ranges(patch: &str) -> Vec<(u32, u32)> {
    patch
        .lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("@@ ")?;
            let plus = rest.split_whitespace().find(|t| t.starts_with('+'))?;
            let mut it = plus.trim_start_matches('+').split(',');
            let start: u32 = it.next()?.parse().ok()?;
            let len: u32 = it.next().map_or(Some(1), |n| n.parse().ok())?;
            (len > 0).then(|| (start, start + len - 1))
        })
        .collect()
}

impl Task for VerifyTests {
    fn id(&self) -> &'static str {
        "verify_tests"
    }
    fn summary(&self) -> &'static str {
        "on demand: judge the tests and mocks added in `--diff` before they land"
    }
    fn on_demand(&self) -> bool {
        true
    }
    fn criteria_text(&self) -> String {
        format!(
            "{}\n{}",
            TestValue.criteria_text(),
            MockScope.criteria_text()
        )
    }
    fn setup_hint(&self, _ctx: &TaskContext<'_>) -> Option<String> {
        Some("pass --diff <range>, e.g. --diff HEAD~1..HEAD".to_owned())
    }
    fn plan(&self, ctx: &TaskContext<'_>) -> anyhow::Result<Vec<Group>> {
        let Some(range) = ctx.diff_range else {
            return Ok(Vec::new());
        };
        let (files, _) = crate::semantic::tasks::verify::range_diff(ctx, range)?;
        let tests = TestMatcher::new(ctx);
        let mut groups = Vec::new();
        for (file, patch) in files {
            if !tests.is_test(&file) {
                continue;
            }
            let added = added_ranges(&patch);
            let touches = |s: u32, e: u32| added.iter().any(|(a, b)| s <= *b && *a <= e);
            let Some(parsed) = parse_file(ctx, &file) else {
                continue;
            };
            let cases: Vec<TestCase> = test_cases(&parsed)
                .into_iter()
                .filter(|c| touches(c.start_line, c.end_line))
                .collect();
            if !cases.is_empty() {
                groups.extend(value_groups(self, ctx, &file, &parsed.source, &cases));
            }
            let sites: Vec<(u32, String)> = mock_sites(&parsed.source, parsed.lang)
                .into_iter()
                .filter(|s| touches(s.line, s.line))
                .map(|s| (s.line, s.text))
                .collect();
            if !sites.is_empty() {
                let spans: Vec<(u32, u32)> = sites.iter().map(|(l, _)| (*l, *l)).collect();
                for (state, covered) in test_states(ctx, &file, &parsed.source, &spans) {
                    let lines: Vec<(u32, String)> =
                        covered.iter().map(|&i| sites[i].clone()).collect();
                    let mut items = mock_items(self, ctx, &file, &state, &lines);
                    for it in &mut items {
                        it.meta["q"] = json!("mock");
                    }
                    groups.push(Group {
                        items,
                        state: json!(state),
                    });
                }
            }
        }
        Ok(groups)
    }
    fn report(&self, ctx: &TaskContext<'_>, answered: &[Answered<'_>]) -> Option<Value> {
        let (mocks, cases): (Vec<&Answered<'_>>, Vec<&Answered<'_>>) =
            answered.iter().partition(|a| a.item.meta["q"] == "mock");
        let cases_owned: Vec<Answered<'_>> = cases
            .iter()
            .map(|a| Answered {
                item: a.item,
                answer: a.answer,
            })
            .collect();
        let mut rows = Vec::new();
        let mut failing = BTreeSet::new();
        for ((file, test, start), v) in collect4(&cases_owned) {
            let label = v.label();
            if label.is_some() {
                failing.insert(format!("{file}:{test}"));
            }
            rows.push(json!({"file": file, "test": test, "line": start, "label": label}));
        }
        let cutoff = ctx.cutoff(self, 0.6);
        let mut mock_rows = Vec::new();
        for a in mocks {
            let Some((kind, p, _)) = a.answer.and_then(chosen) else {
                continue;
            };
            let bad = kind != "boundary" && p >= cutoff;
            if bad {
                failing.insert(format!(
                    "{}:{}",
                    a.item.meta["file"].as_str().unwrap_or(""),
                    a.item.meta["line"]
                ));
            }
            mock_rows.push(json!({"file": a.item.meta["file"], "line": a.item.meta["line"], "kind": kind, "p": p, "bad": bad}));
        }
        let unanswered = answered.iter().filter(|a| a.answer.is_none()).count();
        Some(json!({
            "range": ctx.diff_range,
            "tests": rows,
            "mocks": mock_rows,
            "pass": failing.is_empty() && unanswered == 0,
            "unanswered": unanswered,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_rewrite_keep() {
        let v = Verdict4 {
            regression: Some(0.1),
            brittle: Some(0.2),
            checks: Some(("mock_values".into(), 0.95, 0.95)),
            has_logic: Some(0.9),
        };
        assert_eq!(v.label(), Some("delete"));
        let v = Verdict4 {
            regression: Some(0.9),
            brittle: Some(0.8),
            checks: Some(("behaviour".into(), 0.9, 0.9)),
            has_logic: Some(0.9),
        };
        assert_eq!(v.label(), Some("rewrite"));
        let v = Verdict4 {
            regression: Some(0.9),
            brittle: Some(0.1),
            checks: Some(("behaviour".into(), 0.9, 0.9)),
            has_logic: Some(0.9),
        };
        assert_eq!(v.label(), None);
        // A trivial-looking answer below 0.9 confidence never deletes.
        let v = Verdict4 {
            regression: Some(0.1),
            brittle: None,
            checks: Some(("nothing".into(), 0.8, 0.8)),
            has_logic: Some(0.9),
        };
        assert_eq!(v.label(), None);
    }

    #[test]
    fn added_ranges_parse_hunk_headers() {
        let patch = "@@ -1,3 +1,4 @@\n x\n+y\n@@ -10 +11,0 @@\n-z\n@@ -20,2 +22 @@\n";
        assert_eq!(added_ranges(patch), [(1, 4), (22, 22)]);
    }
}
