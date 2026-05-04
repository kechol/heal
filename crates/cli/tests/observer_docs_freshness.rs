//! Integration coverage for `DocFreshnessObserver`: tempdir-backed git
//! fixtures driving the per-pair "src commits since paired doc" count.

use std::time::{SystemTime, UNIX_EPOCH};

use heal_cli::core::doc_pairs::{DocPair, PairSource};
use heal_cli::core::finding::IntoFindings;
use heal_cli::core::severity::Severity;
use heal_cli::observer::docs::freshness::DocFreshnessObserver;

mod common;
use common::{commit_files, init_repo};

fn now_secs() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap()
}

fn pair(doc: &str, srcs: &[&str]) -> DocPair {
    DocPair {
        doc: doc.to_owned(),
        srcs: srcs.iter().map(|s| (*s).to_owned()).collect(),
        confidence: None,
        source: Some(PairSource::Manual),
    }
}

fn observer_with(pairs: Vec<DocPair>) -> DocFreshnessObserver {
    DocFreshnessObserver {
        enabled: true,
        pairs,
        high_commits: 5,
        critical_commits: 20,
    }
}

#[test]
fn empty_when_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let observer = DocFreshnessObserver {
        enabled: false,
        ..observer_with(vec![pair("docs/cli.md", &["src/cli.rs"])])
    };
    let report = observer.scan(dir.path());
    assert!(report.entries.is_empty());
}

#[test]
fn empty_when_no_pairs() {
    let dir = tempfile::tempdir().unwrap();
    let _repo = init_repo(dir.path());
    let report = observer_with(vec![]).scan(dir.path());
    assert!(report.entries.is_empty());
    assert_eq!(report.totals.pairs, 0);
}

#[test]
fn fresh_pair_has_no_src_commits_since_doc() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();

    // One commit touching both doc and src — they share a timestamp,
    // so src has zero commits *strictly after* the doc.
    commit_files(
        &repo,
        &[("docs/cli.md", "# CLI\n"), ("src/cli.rs", "fn main() {}\n")],
        "init",
        now - 100,
    );

    let report = observer_with(vec![pair("docs/cli.md", &["src/cli.rs"])]).scan(dir.path());
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].src_commits_since_doc, 0);
    assert_eq!(report.totals.stale_pairs, 0);
}

#[test]
fn counts_src_commits_after_doc_last_commit() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();

    // Init with both files together.
    commit_files(
        &repo,
        &[("docs/cli.md", "# CLI\n"), ("src/cli.rs", "fn main() {}\n")],
        "init",
        now - 100,
    );
    // Three src-only commits past the doc's last commit.
    commit_files(
        &repo,
        &[("src/cli.rs", "fn main() { 1; }\n")],
        "src 1",
        now - 80,
    );
    commit_files(
        &repo,
        &[("src/cli.rs", "fn main() { 1;2; }\n")],
        "src 2",
        now - 60,
    );
    commit_files(
        &repo,
        &[("src/cli.rs", "fn main() { 1;2;3; }\n")],
        "src 3",
        now - 40,
    );

    let report = observer_with(vec![pair("docs/cli.md", &["src/cli.rs"])]).scan(dir.path());
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].src_commits_since_doc, 3);
    assert_eq!(report.totals.stale_pairs, 1);
}

#[test]
fn doc_only_commits_reset_the_clock() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();

    // Two src commits, then a doc commit, then no more src commits.
    // Src moved 2 times before the doc was last touched, but 0 times
    // since — the metric is "since paired doc last changed" so the
    // result is 0.
    commit_files(&repo, &[("src/cli.rs", "v1\n")], "src 1", now - 80);
    commit_files(&repo, &[("src/cli.rs", "v2\n")], "src 2", now - 70);
    commit_files(&repo, &[("docs/cli.md", "v1\n")], "doc 1", now - 60);

    let report = observer_with(vec![pair("docs/cli.md", &["src/cli.rs"])]).scan(dir.path());
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].src_commits_since_doc, 0);
}

#[test]
fn multi_src_pair_collapses_co_modified_commits() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();

    commit_files(
        &repo,
        &[
            ("docs/cli.md", "# CLI\n"),
            ("src/a.rs", "v1\n"),
            ("src/b.rs", "v1\n"),
        ],
        "init",
        now - 100,
    );
    // One commit touching both srcs — collapses to a single bump.
    commit_files(
        &repo,
        &[("src/a.rs", "v2\n"), ("src/b.rs", "v2\n")],
        "co",
        now - 50,
    );
    // Then a single-src commit.
    commit_files(&repo, &[("src/a.rs", "v3\n")], "a only", now - 30);

    let report =
        observer_with(vec![pair("docs/cli.md", &["src/a.rs", "src/b.rs"])]).scan(dir.path());
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].src_commits_since_doc, 2);
}

#[test]
fn classify_assigns_severity_against_floors() {
    let observer = observer_with(vec![]);
    assert_eq!(observer.classify(0), Severity::Ok);
    assert_eq!(observer.classify(1), Severity::Medium);
    assert_eq!(observer.classify(4), Severity::Medium);
    assert_eq!(observer.classify(5), Severity::High);
    assert_eq!(observer.classify(19), Severity::High);
    assert_eq!(observer.classify(20), Severity::Critical);
    assert_eq!(observer.classify(100), Severity::Critical);
}

#[test]
fn into_findings_skips_fresh_pairs() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();
    commit_files(
        &repo,
        &[
            ("docs/a.md", "v1\n"),
            ("docs/b.md", "v1\n"),
            ("src/a.rs", "v1\n"),
            ("src/b.rs", "v1\n"),
        ],
        "init",
        now - 200,
    );
    // Only b drifts.
    commit_files(&repo, &[("src/b.rs", "v2\n")], "b drift", now - 100);

    let observer = observer_with(vec![
        pair("docs/a.md", &["src/a.rs"]),
        pair("docs/b.md", &["src/b.rs"]),
    ]);
    let report = observer.scan(dir.path());
    let findings = report.into_findings();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].metric, "doc_freshness");
    assert_eq!(findings[0].location.file.to_string_lossy(), "docs/b.md");
}
