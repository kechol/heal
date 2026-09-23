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

/// Classifies paths as tests for the semantic tasks.
///
/// With the default `[features.test].test_paths`, the naming heuristic
/// (`is_test_path`) is added to the globs, because the default globs are
/// root-anchored (`tests/**`) and miss nested test directories such as
/// `crates/<name>/tests/`. Once a project sets its own `test_paths`, those
/// globs alone decide: the heuristic also matches any `test/` directory,
/// including production code such as a `src/observer/test/` module, and
/// the project's explicit globs are the only way to correct that.
pub struct TestMatcher {
    glob: Option<ExcludeMatcher>,
    heuristic: bool,
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
        let heuristic =
            glob.is_none() || *paths == crate::core::config::TestConfig::default().test_paths;
        Self { glob, heuristic }
    }

    #[must_use]
    pub fn is_test(&self, path: &Path) -> bool {
        (self.heuristic && is_test_path(path))
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

/// Per-level probabilities of a `score` answer, indexed by level. When
/// the server sent none (older verdicts, test fakes), all mass goes to the
/// rounded `score`.
#[must_use]
pub fn level_probs(answer: &Answer, levels: usize) -> Option<Vec<f64>> {
    let Answer::Score {
        score,
        probabilities,
        ..
    } = answer
    else {
        return None;
    };
    if levels == 0 {
        return None;
    }
    let mut p = vec![0.0; levels];
    if probabilities.is_empty() {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i = (score.round().max(0.0) as usize).min(levels.saturating_sub(1));
        p[i] = 1.0;
    } else {
        for (level, prob) in probabilities {
            if let Some(slot) = level.parse::<usize>().ok().and_then(|i| p.get_mut(i)) {
                *slot = *prob;
            }
        }
    }
    Some(p)
}

/// For a rubric whose level 0 means "not applicable" and whose other
/// levels run from clean to worst: `(P(applies), P(level >= from |
/// applies))`.
///
/// Such rubrics must not be read through `score`. The server's `score` is
/// the probability-weighted mean over every level, so an "n/a" share drags
/// it toward 0, and an answer split between "n/a" and the worst level
/// reads as a middle one (measured: P(n/a) = 0.34, P(worst) = 0.34,
/// P(level 2) = 0.20 gives `score` 1.52, below "partly wrong").
#[must_use]
pub fn applicable_share(answer: &Answer, levels: usize, from: usize) -> Option<(f64, f64)> {
    let p = level_probs(answer, levels)?;
    let applies: f64 = p.iter().skip(1).sum();
    if applies <= 0.0 {
        return Some((0.0, 0.0));
    }
    let high: f64 = p.iter().skip(from.max(1)).sum();
    Some((applies, (high / applies).clamp(0.0, 1.0)))
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
            line.saturating_add(40),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn score_with(probs: &[(&str, f64)]) -> Answer {
        let probabilities: BTreeMap<String, f64> =
            probs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect();
        let score = probabilities
            .iter()
            .map(|(k, v)| k.parse::<f64>().unwrap() * v)
            .sum();
        Answer::Score {
            score,
            probabilities,
            confidence: 0.0,
        }
    }

    #[test]
    fn applicable_share_ignores_the_not_applicable_level() {
        // Recorded from the live API: the mean reads 1.52 ("accurate"),
        // yet two thirds of the applicable mass says outdated or wrong.
        let a = score_with(&[("0", 0.34), ("1", 0.12), ("2", 0.20), ("3", 0.34)]);
        let (applies, stale) = applicable_share(&a, 4, 2).unwrap();
        assert!((applies - 0.66).abs() < 1e-9);
        assert!((stale - 0.54 / 0.66).abs() < 1e-9);
        let (_, wrong) = applicable_share(&a, 4, 3).unwrap();
        assert!((wrong - 0.34 / 0.66).abs() < 1e-9);

        // Mostly "not applicable": applies < 0.5 whatever the rest says.
        let b = score_with(&[("0", 0.61), ("1", 0.03), ("2", 0.01), ("3", 0.35)]);
        assert!(applicable_share(&b, 4, 2).unwrap().0 < 0.5);
    }

    #[test]
    fn explicit_test_paths_replace_the_naming_heuristic() {
        let module = Path::new("crates/cli/src/observer/test/cases.rs");
        let nested = Path::new("crates/cli/tests/core_config.rs");

        let mut cfg = crate::core::config::Config::default();
        let ctx = TaskContext::new(Path::new("."), &cfg).unwrap();
        let default = TestMatcher::new(&ctx);
        assert!(default.is_test(nested), "heuristic finds nested tests");
        assert!(
            default.is_test(module),
            "heuristic also claims a `test/` module"
        );

        cfg.features.test.test_paths = vec!["**/tests/**".to_owned()];
        let ctx = TaskContext::new(Path::new("."), &cfg).unwrap();
        let explicit = TestMatcher::new(&ctx);
        assert!(explicit.is_test(nested));
        assert!(!explicit.is_test(module));
    }

    #[test]
    fn level_probs_fall_back_to_the_rounded_score() {
        let a = Answer::Score {
            score: 2.6,
            probabilities: BTreeMap::new(),
            confidence: 0.9,
        };
        assert_eq!(level_probs(&a, 4).unwrap(), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(applicable_share(&a, 4, 3), Some((1.0, 1.0)));
        assert!(level_probs(&Answer::Noul { noul: 0.5 }, 4).is_none());
    }
}
