//! Helpers shared by the semantic tasks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::core::finding::{Finding, Location, SemanticNote};
use crate::core::severity::Severity;
use crate::observer::code::complexity::{parse, ParsedFile};
use crate::observer::shared::file_role::is_test_path;
use crate::observer::shared::lang::Language;
use crate::observer::shared::walk::ExcludeMatcher;
use crate::semantic::api::Answer;
use crate::semantic::task::TaskContext;

/// Classifies paths as tests: `[features.test].test_paths`, or the naming
/// heuristic. Unlike `Feature::lower`'s `is_test_file` tag (globs only when
/// set), the heuristic always applies here, because the default globs
/// are root-anchored (`tests/**`) and miss nested test directories such as
/// `crates/<name>/tests/` — sending a test file to a production-code
/// question would waste the question and skew the concept map.
pub struct TestMatcher {
    glob: Option<ExcludeMatcher>,
}

impl TestMatcher {
    #[must_use]
    pub fn new(ctx: &TaskContext<'_>) -> Self {
        let paths = &ctx.config.features.test.test_paths;
        let glob = if paths.is_empty() {
            None
        } else {
            ExcludeMatcher::compile(Path::new(""), paths).ok()
        };
        Self { glob }
    }

    #[must_use]
    pub fn is_test(&self, path: &Path) -> bool {
        is_test_path(path)
            || self
                .glob
                .as_ref()
                .is_some_and(|m| m.is_excluded(path, false))
    }
}

/// Source files the observers parsed, split into (production, tests),
/// each sorted and filtered to what may be sent.
#[must_use]
pub fn code_files(ctx: &TaskContext<'_>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let Some(reports) = ctx.reports else {
        return (Vec::new(), Vec::new());
    };
    let tests = TestMatcher::new(ctx);
    let mut prod = Vec::new();
    let mut test = Vec::new();
    for f in &reports.complexity.files {
        if !ctx.may_send(&f.path) {
            continue;
        }
        if tests.is_test(&f.path) {
            test.push(f.path.clone());
        } else {
            prod.push(f.path.clone());
        }
    }
    prod.sort();
    prod.dedup();
    test.sort();
    test.dedup();
    (prod, test)
}

/// Read and parse a project-relative file, if it may be sent.
#[must_use]
pub fn parse_file(ctx: &TaskContext<'_>, rel: &Path) -> Option<ParsedFile> {
    let lang = Language::from_path(rel)?;
    let source = ctx.read_sendable(rel)?;
    parse(source, lang).ok()
}

/// Source text with 1-based line numbers, so questions can point at
/// "lines 12–40" and the model can find them in the state.
#[must_use]
pub fn numbered(source: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(source.len() + source.len() / 8);
    for (i, line) in source.lines().enumerate() {
        let _ = writeln!(out, "{:>5}| {line}", i + 1);
    }
    out
}

/// Lines `from..=to` (1-based, clamped) with their original numbers.
#[must_use]
pub fn numbered_range(source: &str, from: u32, to: u32) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for (i, line) in source.lines().enumerate() {
        let n = u32::try_from(i + 1).unwrap_or(u32::MAX);
        if n < from {
            continue;
        }
        if n > to {
            break;
        }
        let _ = writeln!(out, "{n:>5}| {line}");
    }
    out
}

/// Lines of context kept around a subject when its file is too large to
/// send whole.
const WINDOW_CONTEXT: u32 = 40;

/// The state for questions about spans of one file: the whole numbered
/// file when it fits the state budget, else one window per span
/// (the span ± 40 lines). Returns `(state, indices of spans it covers)`.
/// A window that still does not fit is returned anyway; the packer
/// counts it as oversized rather than truncating the code silently.
#[must_use]
pub fn file_states(rel: &Path, source: &str, spans: &[(u32, u32)]) -> Vec<(String, Vec<usize>)> {
    let whole = format!("File: {}\n\n{}", rel.display(), numbered(source));
    if crate::semantic::cost::estimate_tokens(whole.len()) <= crate::semantic::cost::state_budget()
    {
        return vec![(whole, (0..spans.len()).collect())];
    }
    spans
        .iter()
        .enumerate()
        .map(|(i, (start, end))| {
            let from = start.saturating_sub(WINDOW_CONTEXT).max(1);
            let to = end.saturating_add(WINDOW_CONTEXT);
            (
                format!(
                    "File: {} (excerpt, lines {from}–{to}; the file is too large to send whole)\n\n{}",
                    rel.display(),
                    numbered_range(source, from, to)
                ),
                vec![i],
            )
        })
        .collect()
}

/// `(label, description)` pairs as `choice` criteria.
#[must_use]
pub fn criteria(labels: &[(String, String)]) -> BTreeMap<String, Value> {
    labels.iter().map(|(k, v)| (k.clone(), json!(v))).collect()
}

/// Selected label and its probability for a `choice` answer.
#[must_use]
pub fn chosen(answer: &Answer) -> Option<(&str, f64, f64)> {
    match answer {
        Answer::Choice {
            choice,
            probabilities,
            confidence,
        } => Some((
            choice.as_str(),
            probabilities.get(choice).copied().unwrap_or(0.0),
            *confidence,
        )),
        _ => None,
    }
}

/// A `noul` probability.
#[must_use]
pub fn noul_p(answer: &Answer) -> Option<f64> {
    match answer {
        Answer::Noul { noul } => Some(*noul),
        _ => None,
    }
}

/// Score level (0-based) and confidence of a `score` answer.
#[must_use]
pub fn score_level(answer: &Answer) -> Option<(f64, f64)> {
    match answer {
        Answer::Score {
            score, confidence, ..
        } => Some((*score, *confidence)),
        _ => None,
    }
}

/// A note from an answer.
#[must_use]
pub fn note(label: impl Into<String>, answer: &Answer, levels: usize) -> SemanticNote {
    SemanticNote {
        label: label.into(),
        p: answer.strength(levels),
        confidence: answer.confidence(),
        lines: Vec::new(),
        detail: None,
    }
}

/// Build a semantic Finding. Semantic findings are candidates for a
/// person to judge (a classifier is wrong roughly one time in five on
/// real code, per jev-lint's measurements), so a task never assigns
/// `Critical` on its own; the ceiling is `High`.
#[must_use]
pub fn finding(
    metric: &str,
    file: &Path,
    line: Option<u32>,
    symbol: Option<&str>,
    summary: String,
    seed: &str,
    severity: Severity,
) -> Finding {
    let mut f = Finding::new(
        metric,
        Location {
            file: file.to_path_buf(),
            line,
            symbol: symbol.map(str::to_owned),
        },
        summary,
        seed,
    );
    f.severity = severity.min(Severity::High);
    f
}

/// The code a Finding is about, numbered: its function when the symbol
/// resolves, else ±40 lines around its line, else the file's first 200
/// lines. `None` when the file may not be sent or cannot be read.
#[must_use]
pub fn finding_excerpt(ctx: &TaskContext<'_>, f: &Finding) -> Option<String> {
    let src = ctx.read_sendable(&f.location.file)?;
    if let (Some(symbol), Some(lang)) = (&f.location.symbol, Language::from_path(&f.location.file))
    {
        if let Ok(parsed) = parse(src.clone(), lang) {
            if let Some(func) = crate::observer::code::complexity::outer_functions(&parsed)
                .into_iter()
                .find(|x| &x.name == symbol)
            {
                return Some(numbered_range(&src, func.start_row, func.end_row));
            }
        }
    }
    if let Some(line) = f.location.line {
        return Some(numbered_range(
            &src,
            line.saturating_sub(40).max(1),
            line + 40,
        ));
    }
    Some(numbered_range(&src, 1, 200))
}

/// The production file a test file most likely exercises, by naming
/// convention: `foo_test.rs` / `foo.test.ts` / `foo.spec.js` /
/// `test_foo.py` / `foo_test.go` → `foo.<ext>` next to it, and
/// `tests/foo.rs` / `test/foo.py` / `__tests__/foo.ts` → the same name
/// under `src/` (or the parent directory). Only existing files count.
#[must_use]
pub fn guess_src_for_test(ctx: &TaskContext<'_>, test: &Path) -> Option<PathBuf> {
    let ext = test.extension().and_then(|e| e.to_str()).unwrap_or("");
    let stem = test.file_stem()?.to_str()?;
    let base = stem
        .strip_suffix("_test")
        .or_else(|| stem.strip_suffix(".test"))
        .or_else(|| stem.strip_suffix(".spec"))
        .or_else(|| stem.strip_prefix("test_"))
        .or_else(|| stem.strip_suffix("Test"))
        .or_else(|| stem.strip_suffix("Spec"))
        .unwrap_or(stem);
    let file = if ext.is_empty() {
        base.to_owned()
    } else {
        format!("{base}.{ext}")
    };
    let dir = test.parent().unwrap_or_else(|| Path::new(""));
    let mut candidates = vec![dir.join(&file)];
    let parts: Vec<&str> = dir.iter().filter_map(|c| c.to_str()).collect();
    if let Some(i) = parts
        .iter()
        .position(|p| matches!(*p, "tests" | "test" | "__tests__" | "spec"))
    {
        let mut prefix: PathBuf = parts[..i].iter().collect();
        let rest: PathBuf = parts[i + 1..].iter().collect();
        candidates.push(prefix.join("src").join(&rest).join(&file));
        candidates.push(prefix.join(&rest).join(&file));
        prefix.push(&file);
        candidates.push(prefix);
    }
    candidates
        .into_iter()
        .find(|c| c.as_path() != test && ctx.project.join(c).is_file())
}

/// Findings of the ordinary families on `file`.
pub fn findings_on<'a>(
    ctx: &'a TaskContext<'_>,
    file: &'a Path,
) -> impl Iterator<Item = &'a Finding> {
    ctx.findings.iter().filter(move |f| f.location.file == file)
}
