//! `skip_ratio` observer (`[features.test]`).
//!
//! Walks the project's test files (filtered by
//! [`crate::core::config::TestConfig::test_paths`]) and, for each file,
//! counts:
//!
//! - **Total tests** — language-specific test definition nodes
//!   (`#[test]` for Rust, `def test_*` for Python, `it()`/`test()` calls
//!   for JS/TS, `func TestFoo` for Go, `ScalaTest` `test()` for Scala).
//! - **Skipped tests** — language-specific skip markers (`#[ignore]`,
//!   `@pytest.mark.skip`, `it.skip` / `xit`, `t.Skip()`,
//!   `ScalaTest` `ignore`).
//!
//! The result is a per-file `skip_pct = skipped / total * 100` that the
//! `Feature::lower` pass classifies against `[calibration.skip_ratio]`.
//! Detection is purely structural — tree-sitter walking with per-language
//! node-kind + identifier-text discrimination — so comments and string
//! literals can't trigger false positives.
//!
//! Languages with simpler skip semantics (Rust, Python, JS/TS, Scala)
//! get exact counts; **Go** is approximate because `t.Skip()` is a
//! runtime call and a single test can contain multiple skip statements
//! — the walker dedupes on the enclosing function declaration.

// `HashSet` is consumed only by the `lang-go` walker (see the
// `cfg(feature = "lang-go")` block below). Gate the import to match
// so a single-language build like `--features lang-javascript`
// doesn't trip `-D unused-imports` in CI.
#[cfg(feature = "lang-go")]
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tree_sitter::Node;

use crate::core::calibration::MetricCalibration;
use crate::core::config::Config;
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::feature::{decorate, Family, Feature, FeatureKind, FeatureMeta, HotspotIndex};
use crate::observer::code::complexity::{parse, ParsedFile};
use crate::observer::shared::lang::Language;
use crate::observer::shared::walk::{walk_supported_files_under, ExcludeMatcher};

/// Fallback calibration used when `[calibration.skip_ratio]` is absent.
/// Anchored at the TODO §[features.test] thresholds: > 1% → Medium, > 5%
/// → High, > 20% → Critical (via floor). `floor_ok = 0.5` graduates "no
/// meaningful skips" to Ok. Spread between p50 and p95 is wide enough
/// (10 vs 0) that the spread gate stays open.
const FALLBACK_CALIBRATION: MetricCalibration = MetricCalibration {
    p50: 0.0,
    p75: 1.0,
    p90: 5.0,
    p95: 10.0,
    floor_critical: Some(20.0),
    floor_ok: Some(0.5),
};

/// Stateless observer. Construction reads the relevant config switches;
/// the `scan` invocation does the I/O.
#[derive(Debug, Clone, Default)]
pub struct SkipRatioObserver {
    pub enabled: bool,
    pub test_paths: Vec<String>,
    pub excluded: Vec<String>,
}

impl SkipRatioObserver {
    #[must_use]
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            enabled: cfg.features.test.enabled,
            test_paths: cfg.features.test.test_paths.clone(),
            excluded: cfg.exclude_lines(),
        }
    }

    #[must_use]
    pub fn scan(&self, root: &Path) -> SkipRatioReport {
        if !self.enabled || self.test_paths.is_empty() {
            return SkipRatioReport::default();
        }
        let exclude = ExcludeMatcher::compile(root, &self.excluded)
            .expect("exclude patterns validated at config load");
        // `ExcludeMatcher` is a gitignore matcher; we reuse it as an
        // *include* matcher by checking `is_excluded` (i.e. "matches the
        // glob") — the semantic flip is that test_paths are positive
        // patterns, not exclusions.
        let Ok(include) = ExcludeMatcher::compile(root, &self.test_paths) else {
            return SkipRatioReport::default();
        };
        let mut entries: Vec<SkipRatioEntry> = Vec::new();
        for path in walk_supported_files_under(root, &exclude, None) {
            let rel = path
                .strip_prefix(root)
                .map_or_else(|_| path.clone(), Path::to_path_buf);
            if !include.is_excluded(&rel, false) {
                continue;
            }
            let Some(lang) = Language::from_path(&path) else {
                continue;
            };
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(parsed) = parse(source, lang) else {
                continue;
            };
            let counts = count_skips(&parsed);
            if counts.total_tests == 0 {
                continue;
            }
            #[allow(clippy::cast_precision_loss)]
            let pct = (f64::from(counts.skipped_tests) / f64::from(counts.total_tests)) * 100.0;
            entries.push(SkipRatioEntry {
                path: rel,
                language: lang.name().to_owned(),
                total_tests: counts.total_tests,
                skipped_tests: counts.skipped_tests,
                skip_pct: pct,
            });
        }
        entries.sort_by(|a, b| {
            b.skip_pct
                .partial_cmp(&a.skip_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.path.cmp(&b.path))
        });
        SkipRatioReport { entries }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SkipRatioReport {
    pub entries: Vec<SkipRatioEntry>,
}

impl SkipRatioReport {
    /// Top-N least-healthy files (descending by `skip_pct`). Files with
    /// zero skipped tests are excluded — they aren't actionable. Sorts
    /// internally so callers don't have to rely on the report having
    /// been built by `scan()`.
    #[must_use]
    pub fn worst_n(&self, n: usize) -> Vec<&SkipRatioEntry> {
        let mut top: Vec<&SkipRatioEntry> = self
            .entries
            .iter()
            .filter(|e| e.skipped_tests > 0)
            .collect();
        top.sort_by(|a, b| {
            b.skip_pct
                .partial_cmp(&a.skip_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.path.cmp(&b.path))
        });
        top.truncate(n);
        top
    }

    /// Total number of files with at least one skipped test.
    #[must_use]
    pub fn skipped_file_count(&self) -> usize {
        self.entries.iter().filter(|e| e.skipped_tests > 0).count()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkipRatioEntry {
    pub path: PathBuf,
    pub language: String,
    pub total_tests: u32,
    pub skipped_tests: u32,
    pub skip_pct: f64,
}

impl Eq for SkipRatioEntry {}

#[derive(Debug, Clone, Copy, Default)]
struct SkipCounts {
    total_tests: u32,
    skipped_tests: u32,
}

fn count_skips(parsed: &ParsedFile) -> SkipCounts {
    match parsed.lang {
        #[cfg(feature = "lang-rust")]
        Language::Rust => count_rust(parsed),
        #[cfg(feature = "lang-python")]
        Language::Python => count_python(parsed),
        #[cfg(feature = "lang-typescript")]
        Language::TypeScript | Language::Tsx => count_jsts(parsed),
        #[cfg(feature = "lang-javascript")]
        Language::JavaScript | Language::Jsx => count_jsts(parsed),
        #[cfg(feature = "lang-go")]
        Language::Go => count_go(parsed),
        #[cfg(feature = "lang-scala")]
        Language::Scala => count_scala(parsed),
    }
}

#[cfg(feature = "lang-rust")]
fn count_rust(parsed: &ParsedFile) -> SkipCounts {
    // Rust attributes (`#[test]`, `#[ignore]`) appear as `attribute_item`
    // nodes that are SIBLINGS of the `function_item` they decorate (not
    // children). Counting attribute identifiers directly is the simpler,
    // more robust approach: in real-world Rust, `#[ignore]` only
    // decorates `#[test]` functions, so `skipped <= total` holds.
    let mut counts = SkipCounts::default();
    walk_each_node(parsed.tree.root_node(), &mut |node| {
        if node.kind() != "attribute" {
            return;
        }
        let mut cur = node.walk();
        for child in node.named_children(&mut cur) {
            // The attribute path is either a bare `identifier` (`#[test]`,
            // `#[ignore]`) or a `scoped_identifier` (`#[tokio::test]`,
            // `#[async_std::test]`); for the scoped form the trailing
            // `name` segment is what identifies the test macro.
            let name_node = match child.kind() {
                "identifier" => Some(child),
                "scoped_identifier" => child.child_by_field_name("name"),
                _ => None,
            };
            let Some(name_node) = name_node else {
                continue;
            };
            let text = name_node.utf8_text(parsed.source.as_bytes()).unwrap_or("");
            match text {
                "test" => counts.total_tests += 1,
                "ignore" => counts.skipped_tests += 1,
                _ => {}
            }
            break;
        }
    });
    counts
}

#[cfg(feature = "lang-python")]
fn count_python(parsed: &ParsedFile) -> SkipCounts {
    let mut counts = SkipCounts::default();
    let src = parsed.source.as_bytes();
    walk_each_node(parsed.tree.root_node(), &mut |node| {
        if node.kind() != "function_definition" {
            return;
        }
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Ok(name) = name_node.utf8_text(src) else {
            return;
        };
        if !name.starts_with("test_") {
            return;
        }
        counts.total_tests += 1;
        // Decorators live on the parent `decorated_definition` node.
        let Some(parent) = node.parent() else {
            return;
        };
        if parent.kind() != "decorated_definition" {
            return;
        }
        let mut cur = parent.walk();
        let has_skip = parent.children(&mut cur).any(|child| {
            if child.kind() != "decorator" {
                return false;
            }
            let text = child.utf8_text(src).unwrap_or("");
            // `.contains(".skip")` already covers pytest.mark.skip /
            // .skipif and unittest.skip / .skipIf / .skipUnless because
            // each variant has `.skip` as a prefix substring. Only
            // .expectedFailure needs a separate arm.
            text.contains(".skip") || text.contains(".expectedFailure")
        });
        if has_skip {
            counts.skipped_tests += 1;
        }
    });
    counts
}

#[cfg(any(feature = "lang-typescript", feature = "lang-javascript"))]
fn count_jsts(parsed: &ParsedFile) -> SkipCounts {
    let mut counts = SkipCounts::default();
    let src = parsed.source.as_bytes();
    walk_each_node(parsed.tree.root_node(), &mut |node| {
        if node.kind() != "call_expression" {
            return;
        }
        let Some(callee) = node.child_by_field_name("function") else {
            return;
        };
        match callee.kind() {
            "identifier" => {
                let text = callee.utf8_text(src).unwrap_or("");
                match text {
                    "it" | "test" | "describe" | "context" | "fit" | "fdescribe" | "ftest" => {
                        counts.total_tests += 1;
                    }
                    "xit" | "xtest" | "xdescribe" => {
                        counts.total_tests += 1;
                        counts.skipped_tests += 1;
                    }
                    _ => {}
                }
            }
            "member_expression" => {
                // it.skip / describe.skip / it.only / it.todo etc.
                let Some(obj) = callee.child_by_field_name("object") else {
                    return;
                };
                let Some(prop) = callee.child_by_field_name("property") else {
                    return;
                };
                if obj.kind() != "identifier" {
                    return;
                }
                let obj_text = obj.utf8_text(src).unwrap_or("");
                if !matches!(obj_text, "it" | "test" | "describe" | "context") {
                    return;
                }
                let prop_text = prop.utf8_text(src).unwrap_or("");
                match prop_text {
                    "skip" => {
                        counts.total_tests += 1;
                        counts.skipped_tests += 1;
                    }
                    "only" | "todo" => {
                        // `.only` and `.todo` are still test definitions;
                        // they aren't skips, but they shouldn't be missed
                        // from the denominator either.
                        counts.total_tests += 1;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    });
    counts
}

#[cfg(feature = "lang-go")]
fn count_go(parsed: &ParsedFile) -> SkipCounts {
    let src = parsed.source.as_bytes();
    // First pass: every Test* function declaration's byte range.
    let mut test_fns: Vec<(usize, usize)> = Vec::new();
    walk_each_node(parsed.tree.root_node(), &mut |node| {
        if node.kind() != "function_declaration" {
            return;
        }
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Ok(name) = name_node.utf8_text(src) else {
            return;
        };
        if name.starts_with("Test") {
            test_fns.push((node.start_byte(), node.end_byte()));
        }
    });
    let total = u32::try_from(test_fns.len()).unwrap_or(u32::MAX);
    if total == 0 {
        return SkipCounts::default();
    }
    // Second pass: every `*.Skip` / `*.SkipNow` / `*.Skipf` selector.
    // For each, find the enclosing Test* function (if any) and record
    // its start byte. Dedupe so multiple Skip calls in one function
    // count as one skipped test.
    let mut skipped_fn_starts: HashSet<usize> = HashSet::new();
    walk_each_node(parsed.tree.root_node(), &mut |node| {
        if node.kind() != "selector_expression" {
            return;
        }
        let Some(field) = node.child_by_field_name("field") else {
            return;
        };
        let text = field.utf8_text(src).unwrap_or("");
        if !matches!(text, "Skip" | "SkipNow" | "Skipf") {
            return;
        }
        let byte = node.start_byte();
        for &(start, end) in &test_fns {
            if byte >= start && byte < end {
                skipped_fn_starts.insert(start);
                break;
            }
        }
    });
    SkipCounts {
        total_tests: total,
        skipped_tests: u32::try_from(skipped_fn_starts.len()).unwrap_or(u32::MAX),
    }
}

#[cfg(feature = "lang-scala")]
fn count_scala(parsed: &ParsedFile) -> SkipCounts {
    // ScalaTest's API uses bare identifier calls — `test("...") { ... }`
    // and `ignore("...") { ... }`. Both render as `call_expression` with
    // an `identifier` callee in tree-sitter-scala.
    let mut counts = SkipCounts::default();
    let src = parsed.source.as_bytes();
    walk_each_node(parsed.tree.root_node(), &mut |node| {
        if node.kind() != "call_expression" {
            return;
        }
        // tree-sitter-scala exposes the callee as the first named child
        // when no `function:` field is present; try the field first for
        // forward compatibility, then fall back.
        let callee = node
            .child_by_field_name("function")
            .or_else(|| node.named_child(0));
        let Some(callee) = callee else { return };
        if callee.kind() != "identifier" {
            return;
        }
        let text = callee.utf8_text(src).unwrap_or("");
        match text {
            "test" | "it" | "they" | "scenario" => counts.total_tests += 1,
            "ignore" | "pending" => {
                counts.total_tests += 1;
                counts.skipped_tests += 1;
            }
            _ => {}
        }
    });
    counts
}

/// Recursive AST traversal — visits every descendant of `root`,
/// including `root` itself, in pre-order. Real-world tree-sitter
/// trees max out around ~50 levels deep, so plain recursion is fine.
fn walk_each_node<F: FnMut(Node<'_>)>(root: Node<'_>, visit: &mut F) {
    visit(root);
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        walk_each_node(child, visit);
    }
}

impl IntoFindings for SkipRatioReport {
    fn into_findings(&self) -> Vec<Finding> {
        self.entries
            .iter()
            .filter(|e| e.skipped_tests > 0)
            .map(entry_finding)
            .collect()
    }
}

fn entry_finding(entry: &SkipRatioEntry) -> Finding {
    let primary = Location::file(entry.path.clone());
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pct_int = entry.skip_pct.round() as u32;
    let summary = format!(
        "Skip={pct_int}% ({}/{} tests skipped)",
        entry.skipped_tests, entry.total_tests,
    );
    // Seed on the integer percentage so cosmetic reformatting that
    // doesn't change the ratio keeps the finding id stable.
    let seed = format!("skip_ratio:{pct_int}");
    Finding::new(Finding::METRIC_SKIP_RATIO, primary, summary, &seed)
}

pub struct SkipRatioFeature;

impl Feature for SkipRatioFeature {
    fn meta(&self) -> FeatureMeta {
        FeatureMeta {
            name: "skip_ratio",
            version: 1,
            kind: FeatureKind::Observer,
        }
    }
    fn enabled(&self, cfg: &Config) -> bool {
        cfg.features.test.enabled
    }
    fn family(&self) -> Family {
        Family::Test
    }
    fn lower(
        &self,
        reports: &crate::observers::ObserverReports,
        _cfg: &Config,
        cal: &crate::core::calibration::Calibration,
        hotspot: &HotspotIndex,
    ) -> Vec<Finding> {
        let Some(report) = reports.skip_ratio.as_ref() else {
            return Vec::new();
        };
        let calibration = cal
            .calibration
            .skip_ratio
            .as_ref()
            .unwrap_or(&FALLBACK_CALIBRATION);
        report
            .entries
            .iter()
            .filter(|e| e.skipped_tests > 0)
            .map(|entry| {
                decorate(
                    entry_finding(entry),
                    calibration.classify(entry.skip_pct),
                    hotspot,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `std::fs`, `TempDir`, and `cfg_enabled` are consumed only by
    // the `lang-rust` integration tests below. Single-language CI
    // builds (e.g. `--features lang-javascript`) compile this module
    // without those tests, so the imports / helper would trip
    // `-D unused-imports` / `-D dead-code` without these gates.
    #[cfg(feature = "lang-rust")]
    use crate::core::config::TestConfig;
    #[cfg(feature = "lang-rust")]
    use std::fs;
    #[cfg(feature = "lang-rust")]
    use tempfile::TempDir;

    #[cfg(feature = "lang-rust")]
    fn cfg_enabled() -> Config {
        let mut cfg = Config::default();
        cfg.features.test = TestConfig {
            enabled: true,
            ..TestConfig::default()
        };
        cfg
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn rust_counts_test_and_ignore_attributes() {
        let parsed = parse(
            "
                #[test]
                fn ok_test() {}

                #[test]
                #[ignore]
                fn skipped_test() {}

                #[ignore]
                #[test]
                fn skipped_test_2() {}

                fn not_a_test() {}
            "
            .to_owned(),
            Language::Rust,
        )
        .unwrap();
        let counts = count_rust(&parsed);
        assert_eq!(counts.total_tests, 3);
        assert_eq!(counts.skipped_tests, 2);
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn rust_counts_scoped_test_attributes() {
        // `#[tokio::test]` / `#[async_std::test]` parse as
        // `scoped_identifier` attribute paths, not bare `identifier`s.
        // Missing them undercounts `total_tests` while sibling
        // `#[ignore]`s still count, which can push skip_pct past 100%.
        let parsed = parse(
            "
                #[tokio::test]
                async fn async_ok() {}

                #[tokio::test]
                #[ignore]
                async fn async_skipped() {}

                #[async_std::test]
                async fn async_std_ok() {}

                #[test]
                fn plain_ok() {}
            "
            .to_owned(),
            Language::Rust,
        )
        .unwrap();
        let counts = count_rust(&parsed);
        assert_eq!(counts.total_tests, 4);
        assert_eq!(counts.skipped_tests, 1);
    }

    #[cfg(feature = "lang-python")]
    #[test]
    fn python_counts_test_functions_and_skip_decorators() {
        let parsed = parse(
            r#"
import pytest, unittest

def test_one(): pass

@pytest.mark.skip("reason")
def test_skipped(): pass

@pytest.mark.skipif(True, reason="x")
def test_conditional(): pass

@unittest.skipUnless(False, "x")
def test_unless(): pass

def helper(): pass
"#
            .to_owned(),
            Language::Python,
        )
        .unwrap();
        let counts = count_python(&parsed);
        assert_eq!(counts.total_tests, 4);
        assert_eq!(counts.skipped_tests, 3);
    }

    #[cfg(feature = "lang-typescript")]
    #[test]
    fn typescript_counts_it_and_skip_variants() {
        let parsed = parse(
            r#"
                describe("group", () => {
                    it("normal", () => {});
                    it.skip("skipped", () => {});
                    xit("also skipped", () => {});
                    test.only("focused", () => {});
                });
            "#
            .to_owned(),
            Language::TypeScript,
        )
        .unwrap();
        let counts = count_jsts(&parsed);
        // describe + it + it.skip + xit + test.only = 5 tests; 2 skipped.
        assert_eq!(counts.total_tests, 5);
        assert_eq!(counts.skipped_tests, 2);
    }

    #[cfg(feature = "lang-go")]
    #[test]
    fn go_counts_test_funcs_and_dedupes_multiple_skips() {
        let parsed = parse(
            r#"
package foo

import "testing"

func TestOne(t *testing.T) {}

func TestSkipped(t *testing.T) {
    if true {
        t.Skip("reason")
    }
    t.SkipNow()
}

func TestAnotherSkip(t *testing.T) {
    t.Skipf("formatted %d", 1)
}

func helper() {}
"#
            .to_owned(),
            Language::Go,
        )
        .unwrap();
        let counts = count_go(&parsed);
        assert_eq!(counts.total_tests, 3);
        // Two skip calls in TestSkipped collapse to one entry.
        assert_eq!(counts.skipped_tests, 2);
    }

    #[test]
    fn entry_finding_summary_carries_pct() {
        let entry = SkipRatioEntry {
            path: PathBuf::from("tests/foo.rs"),
            language: "rust".into(),
            total_tests: 10,
            skipped_tests: 2,
            skip_pct: 20.0,
        };
        let f = entry_finding(&entry);
        assert!(f.summary.starts_with("Skip=20%"));
        assert_eq!(f.metric, Finding::METRIC_SKIP_RATIO);
    }

    #[test]
    fn into_findings_skips_files_with_zero_skips() {
        let report = SkipRatioReport {
            entries: vec![
                SkipRatioEntry {
                    path: PathBuf::from("tests/clean.rs"),
                    language: "rust".into(),
                    total_tests: 5,
                    skipped_tests: 0,
                    skip_pct: 0.0,
                },
                SkipRatioEntry {
                    path: PathBuf::from("tests/dirty.rs"),
                    language: "rust".into(),
                    total_tests: 5,
                    skipped_tests: 1,
                    skip_pct: 20.0,
                },
            ],
        };
        let findings = report.into_findings();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].location.file, PathBuf::from("tests/dirty.rs"));
    }

    #[test]
    fn fallback_calibration_anchors_at_todo_thresholds() {
        use crate::core::severity::Severity;
        // 25% skip → above floor_critical (20).
        assert_eq!(FALLBACK_CALIBRATION.classify(25.0), Severity::Critical);
        // 0.1% skip → below floor_ok (0.5).
        assert_eq!(FALLBACK_CALIBRATION.classify(0.1), Severity::Ok);
        // 6% skip → above p90 (5) but below p95 (10) → High.
        assert_eq!(FALLBACK_CALIBRATION.classify(6.0), Severity::High);
        // 2% skip → above p75 (1) but below p90 (5) → Medium.
        assert_eq!(FALLBACK_CALIBRATION.classify(2.0), Severity::Medium);
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn observer_disabled_returns_empty_report() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("tests")).unwrap();
        fs::write(
            tmp.path().join("tests/foo.rs"),
            "#[test] fn t() {} #[test] #[ignore] fn s() {}",
        )
        .unwrap();
        let cfg = Config::default(); // features.test disabled
        let report = SkipRatioObserver::from_config(&cfg).scan(tmp.path());
        assert!(report.entries.is_empty());
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn observer_emits_entry_for_test_file_with_skips() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("tests")).unwrap();
        fs::write(
            tmp.path().join("tests/foo.rs"),
            "#[test] fn a() {} #[test] fn b() {} #[test] #[ignore] fn c() {}",
        )
        .unwrap();
        let cfg = cfg_enabled();
        let report = SkipRatioObserver::from_config(&cfg).scan(tmp.path());
        assert_eq!(report.entries.len(), 1);
        let entry = &report.entries[0];
        assert_eq!(entry.total_tests, 3);
        assert_eq!(entry.skipped_tests, 1);
        assert!((entry.skip_pct - 100.0 / 3.0).abs() < 1e-6);
    }
}
