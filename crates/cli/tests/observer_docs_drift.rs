//! Integration coverage for `DocDriftObserver`: filesystem-backed
//! tempdirs driving the doc-mention vs. src-AST diff.

use heal_cli::core::doc_pairs::{DocPair, PairSource};
use heal_cli::core::finding::IntoFindings;
use heal_cli::observer::docs::drift::DocDriftObserver;

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

fn observer_with(pairs: Vec<DocPair>) -> DocDriftObserver {
    DocDriftObserver {
        enabled: true,
        pairs,
    }
}

#[test]
fn empty_when_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let observer = DocDriftObserver {
        enabled: false,
        pairs: vec![pair("docs/cli.md", &["src/cli.rs"])],
    };
    let report = observer.scan(dir.path());
    assert!(report.entries.is_empty());
}

#[cfg(feature = "lang-rust")]
#[test]
fn emits_finding_for_dangling_identifier() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/cli.rs", "fn alive() {}\n");
    write(
        dir.path(),
        "docs/cli.md",
        "Use `alive` to do X. Use `gone_for_real` to do Y.\n",
    );

    let report = observer_with(vec![pair("docs/cli.md", &["src/cli.rs"])]).scan(dir.path());
    let names: Vec<String> = report
        .entries
        .iter()
        .map(|e| e.identifier.clone())
        .collect();
    assert_eq!(names, vec!["gone_for_real".to_string()]);
    assert_eq!(report.totals.dangling_identifiers, 1);
}

#[cfg(feature = "lang-rust")]
#[test]
fn skips_identifiers_inside_code_fences() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/cli.rs", "fn alive() {}\n");
    let body = "Use `alive`.\n\n```rust\nfn obsolete_in_fence() {}\n```\n";
    write(dir.path(), "docs/cli.md", body);

    let report = observer_with(vec![pair("docs/cli.md", &["src/cli.rs"])]).scan(dir.path());
    assert!(
        report.entries.is_empty(),
        "fenced identifiers should not surface as drift: {:?}",
        report.entries,
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn into_findings_attach_doc_line_and_secondary_locations() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/cli.rs", "fn alive() {}\n");
    write(
        dir.path(),
        "docs/cli.md",
        "first line\nUse `gone` here.\nlater line\n",
    );

    let report = observer_with(vec![pair("docs/cli.md", &["src/cli.rs"])]).scan(dir.path());
    let findings = report.into_findings();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].metric, "doc_drift");
    assert_eq!(findings[0].location.file.to_string_lossy(), "docs/cli.md");
    assert_eq!(findings[0].location.line, Some(2));
    assert_eq!(findings[0].locations.len(), 1);
    assert_eq!(
        findings[0].locations[0].file.to_string_lossy(),
        "src/cli.rs"
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn ignores_when_identifier_present_in_any_paired_src() {
    let dir = tempfile::tempdir().unwrap();
    // The identifier is in src/b.rs but doc points to both srcs — the
    // observer should not flag it.
    write(dir.path(), "src/a.rs", "fn unrelated() {}\n");
    write(dir.path(), "src/b.rs", "fn shared() {}\n");
    write(dir.path(), "docs/api.md", "See `shared`.\n");

    let report =
        observer_with(vec![pair("docs/api.md", &["src/a.rs", "src/b.rs"])]).scan(dir.path());
    assert!(report.entries.is_empty());
}

#[test]
fn unsupported_src_extensions_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    // .txt has no tree-sitter grammar bound — the observer should
    // skip it without erroring out, and the doc identifier becomes
    // dangling because no src AST contributed.
    write(dir.path(), "src/notes.txt", "alive\n");
    write(dir.path(), "docs/notes.md", "Use `alive`.\n");

    let report = observer_with(vec![pair("docs/notes.md", &["src/notes.txt"])]).scan(dir.path());
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].identifier, "alive");
}
