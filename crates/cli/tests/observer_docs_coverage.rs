//! Integration coverage for `DocCoverageObserver`: missing-doc surfacing
//! against tempdir layouts.

use heal_cli::core::doc_pairs::{DocPair, PairSource};
use heal_cli::core::finding::IntoFindings;
use heal_cli::observer::docs::coverage::DocCoverageObserver;

mod common;
use common::write;

fn pair(doc: &str, srcs: &[&str]) -> DocPair {
    DocPair {
        doc: doc.to_owned(),
        srcs: srcs.iter().map(|s| (*s).to_owned()).collect(),
        confidence: None,
        source: Some(PairSource::Manual),
    }
}

fn observer_with(pairs: Vec<DocPair>) -> DocCoverageObserver {
    DocCoverageObserver {
        enabled: true,
        pairs,
    }
}

#[test]
fn empty_when_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let observer = DocCoverageObserver {
        enabled: false,
        pairs: vec![pair("docs/cli.md", &["src/cli.rs"])],
    };
    assert!(observer.scan(dir.path()).missing.is_empty());
}

#[test]
fn emits_finding_when_doc_missing_but_src_present() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/cli.rs", "fn main() {}\n");
    // docs/cli.md intentionally absent.
    let report = observer_with(vec![pair("docs/cli.md", &["src/cli.rs"])]).scan(dir.path());
    assert_eq!(report.totals.tracked_srcs, 1);
    assert_eq!(report.totals.missing_docs, 1);
    assert_eq!(report.missing.len(), 1);
    assert_eq!(report.missing[0].src_path.to_string_lossy(), "src/cli.rs");
    assert_eq!(
        report.missing[0].expected_doc_path.to_string_lossy(),
        "docs/cli.md"
    );
}

#[test]
fn no_finding_when_doc_present() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/cli.rs", "fn main() {}\n");
    write(dir.path(), "docs/cli.md", "# CLI\n");
    let report = observer_with(vec![pair("docs/cli.md", &["src/cli.rs"])]).scan(dir.path());
    assert_eq!(report.totals.missing_docs, 0);
    assert!(report.missing.is_empty());
}

#[test]
fn skips_srcs_that_no_longer_exist() {
    // A src that was deleted is surfaced through the integrity warning
    // at load time, not as a coverage finding. The observer must not
    // emit a doc_coverage entry pointing at a vanished src.
    let dir = tempfile::tempdir().unwrap();
    // Neither file exists.
    let report = observer_with(vec![pair("docs/cli.md", &["src/cli.rs"])]).scan(dir.path());
    assert!(report.missing.is_empty());
}

#[test]
fn into_findings_carries_doc_in_secondary_location() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/cli.rs", "fn main() {}\n");
    let report = observer_with(vec![pair("docs/cli.md", &["src/cli.rs"])]).scan(dir.path());
    let findings = report.into_findings();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].metric, "doc_coverage");
    assert_eq!(findings[0].location.file.to_string_lossy(), "src/cli.rs");
    assert_eq!(findings[0].locations.len(), 1);
    assert_eq!(
        findings[0].locations[0].file.to_string_lossy(),
        "docs/cli.md"
    );
}
