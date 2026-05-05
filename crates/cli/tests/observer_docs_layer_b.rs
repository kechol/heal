//! Integration coverage for the three Layer B observers
//! (`doc_link_health`, `orphan_pages`, `todo_density`) plus the shared
//! `walk_standalone_docs` helper.

use std::path::PathBuf;

use heal_cli::core::config::{Config, DocsConfig, FeaturesConfig, TodoDensityConfig};
use heal_cli::core::severity::Severity;
use heal_cli::observer::docs::link_health::{DocLinkHealthObserver, LinkBreakKind};
use heal_cli::observer::docs::orphan_pages::OrphanPagesObserver;
use heal_cli::observer::docs::todo_density::{
    classify as todo_density_classify, TodoDensityObserver,
};
use heal_cli::observer::docs::walk::walk_standalone_docs;

mod common;
use common::write;

fn cfg_with_docs() -> Config {
    Config {
        features: FeaturesConfig {
            docs: DocsConfig {
                enabled: true,
                ..DocsConfig::default()
            },
            ..FeaturesConfig::default()
        },
        ..Config::default()
    }
}

#[test]
fn walk_standalone_docs_picks_md_drops_excluded() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "README.md", "# README\n");
    write(dir.path(), "docs/concept.md", "# Concept\n");
    write(dir.path(), "CHANGELOG.md", "# 1.0\n");
    write(dir.path(), "docs/adr/0001.md", "# ADR\n");
    write(dir.path(), "src/lib.rs", "fn main(){}\n");

    let cfg = cfg_with_docs();
    let docs = walk_standalone_docs(dir.path(), &cfg);
    assert!(docs.contains(&PathBuf::from("README.md")));
    assert!(docs.contains(&PathBuf::from("docs/concept.md")));
    assert!(
        !docs.contains(&PathBuf::from("CHANGELOG.md")),
        "got {docs:?}"
    );
    assert!(
        !docs.contains(&PathBuf::from("docs/adr/0001.md")),
        "got {docs:?}",
    );
}

#[test]
fn doc_link_health_flags_missing_relative_path() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "README.md",
        "see [other](./other.md) and [api](./api.md)\n",
    );
    write(dir.path(), "other.md", "# Other\n");
    // api.md missing.

    let docs = vec![PathBuf::from("README.md"), PathBuf::from("other.md")];
    let cfg = cfg_with_docs();
    let report = DocLinkHealthObserver::from_paths(&cfg, dir.path(), &docs, &[]).scan(dir.path());
    assert_eq!(report.totals.broken, 1);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].target, "./api.md");
    assert!(matches!(report.entries[0].kind, LinkBreakKind::MissingPath));
}

#[test]
fn doc_link_health_flags_missing_anchor_in_same_doc() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "README.md",
        "## Hello World\n\nsee [self](#hello-world) and [bad](#nope)\n",
    );

    let cfg = cfg_with_docs();
    let report =
        DocLinkHealthObserver::from_paths(&cfg, dir.path(), &[PathBuf::from("README.md")], &[])
            .scan(dir.path());
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].target, "#nope");
    assert!(matches!(
        report.entries[0].kind,
        LinkBreakKind::MissingAnchor
    ));
}

#[test]
fn doc_link_health_skips_external_links() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "README.md",
        "see [google](https://google.com) and [mail](mailto:a@b.c)\n",
    );

    let cfg = cfg_with_docs();
    let report =
        DocLinkHealthObserver::from_paths(&cfg, dir.path(), &[PathBuf::from("README.md")], &[])
            .scan(dir.path());
    assert_eq!(report.totals.scanned_links, 0);
    assert!(report.entries.is_empty());
}

#[test]
fn orphan_pages_marks_unlinked_docs() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "README.md", "see [x](./linked.md)\n");
    write(dir.path(), "linked.md", "# Linked\n");
    write(dir.path(), "orphan.md", "# Orphan\n");

    let cfg = cfg_with_docs();
    let docs = vec![
        PathBuf::from("README.md"),
        PathBuf::from("linked.md"),
        PathBuf::from("orphan.md"),
    ];
    let report = OrphanPagesObserver::from_paths(&cfg, dir.path(), &docs, vec![]).scan();
    assert_eq!(report.orphans, vec![PathBuf::from("orphan.md")]);
    assert_eq!(report.totals.orphans, 1);
}

#[test]
fn orphan_pages_treats_paired_docs_as_linked() {
    let dir = tempfile::tempdir().unwrap();
    // Use a non-entry-point name so the orphan check is exercised
    // independently of the README / index seed; docs/cli.md is paired
    // (Layer A) and must NOT show up as orphan even without a link.
    write(dir.path(), "notes.md", "no links here\n");
    write(dir.path(), "docs/cli.md", "# CLI\n");

    let cfg = cfg_with_docs();
    let docs = vec![PathBuf::from("notes.md"), PathBuf::from("docs/cli.md")];
    let report = OrphanPagesObserver::from_paths(
        &cfg,
        dir.path(),
        &docs,
        vec![PathBuf::from("docs/cli.md")],
    )
    .scan();
    assert_eq!(report.orphans, vec![PathBuf::from("notes.md")]);
}

#[test]
fn orphan_pages_treats_readme_as_entry_point() {
    // README.md never counts as an orphan — its reachability comes
    // from outside the doc graph (GitHub repo home, etc.).
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "README.md", "no links here\n");

    let cfg = cfg_with_docs();
    let report =
        OrphanPagesObserver::from_paths(&cfg, dir.path(), &[PathBuf::from("README.md")], vec![])
            .scan();
    assert!(report.orphans.is_empty(), "got: {:?}", report.orphans);
}

#[test]
fn orphan_pages_treats_index_mdx_as_entry_point() {
    // Starlight / Docusaurus / MDX-based SSGs use index.mdx as the
    // landing page; the .md-only check missed it before.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/index.mdx", "# Home\n");

    let cfg = cfg_with_docs();
    let report = OrphanPagesObserver::from_paths(
        &cfg,
        dir.path(),
        &[PathBuf::from("docs/index.mdx")],
        vec![],
    )
    .scan();
    assert!(report.orphans.is_empty(), "got: {:?}", report.orphans);
}

#[test]
fn orphan_pages_honors_configured_entrypoints() {
    // Pages reachable through SSG sidebar configs (Starlight, Hugo,
    // mkdocs nav) need explicit declaration here; nothing else links
    // to them in the body graph.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/quick-start.mdx", "# Quick start\n");
    write(dir.path(), "docs/concept.md", "# Concept\n");
    write(dir.path(), "docs/orphan.md", "# Truly orphaned\n");

    let mut cfg = cfg_with_docs();
    cfg.features.docs.standalone.entrypoints = vec![
        "docs/quick-start.*".to_string(),
        "docs/concept.md".to_string(),
    ];
    let docs = vec![
        PathBuf::from("docs/quick-start.mdx"),
        PathBuf::from("docs/concept.md"),
        PathBuf::from("docs/orphan.md"),
    ];
    let report = OrphanPagesObserver::from_paths(&cfg, dir.path(), &docs, vec![]).scan();
    assert_eq!(report.orphans, vec![PathBuf::from("docs/orphan.md")]);
}

#[test]
fn todo_density_counts_markers_outside_fences() {
    let dir = tempfile::tempdir().unwrap();
    let body = "# Page\n\nTODO refresh.\n\n```\n// TODO inside fence is excluded\n```\n\n[要確認] 仕様未定\nFIXME the link.\n";
    write(dir.path(), "page.md", body);

    let cfg = cfg_with_docs();
    let report =
        TodoDensityObserver::from_paths(&cfg, dir.path(), &[PathBuf::from("page.md")]).scan();
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].marker_count, 3);
}

#[test]
fn todo_density_skips_markers_inside_inline_code_by_default() {
    let dir = tempfile::tempdir().unwrap();
    // Two real markers in prose, plus one mention each inside single-
    // and double-backtick spans (the doc is *quoting* them — it
    // happens to be a reference page describing the observer itself).
    let body = "# Reference\n\nThe observer counts `TODO`, `FIXME`, ``XXX``, and `[要確認]` markers.\n\nReal: TODO refresh.\n[要確認] 仕様未定\n";
    write(dir.path(), "page.md", body);

    let cfg = cfg_with_docs();
    let report =
        TodoDensityObserver::from_paths(&cfg, dir.path(), &[PathBuf::from("page.md")]).scan();
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].marker_count, 2);
}

#[test]
fn todo_density_counts_inline_code_markers_when_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let body = "Reference: `TODO` and `FIXME` markers.\nReal TODO line.\n";
    write(dir.path(), "page.md", body);

    let mut cfg = cfg_with_docs();
    cfg.features.docs.todo_density = TodoDensityConfig {
        ignore_in_inline_code: false,
        allowlist_paths: Vec::new(),
    };
    let report =
        TodoDensityObserver::from_paths(&cfg, dir.path(), &[PathBuf::from("page.md")]).scan();
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].marker_count, 3);
}

#[test]
fn todo_density_allowlist_paths_skips_doc_entirely() {
    let dir = tempfile::tempdir().unwrap();
    let kept = "TODO real action item.\n";
    let allowed = "TODO same shape but documented.\n";
    write(dir.path(), "kept.md", kept);
    write(dir.path(), "docs/allowed.md", allowed);

    let mut cfg = cfg_with_docs();
    cfg.features.docs.todo_density = TodoDensityConfig {
        ignore_in_inline_code: true,
        allowlist_paths: vec!["docs/allowed.md".to_owned()],
    };
    let report = TodoDensityObserver::from_paths(
        &cfg,
        dir.path(),
        &[PathBuf::from("kept.md"), PathBuf::from("docs/allowed.md")],
    )
    .scan();
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].doc_path, PathBuf::from("kept.md"));
}

#[test]
fn todo_density_classify_floors() {
    assert_eq!(todo_density_classify(0), Severity::Ok);
    assert_eq!(todo_density_classify(1), Severity::Ok);
    assert_eq!(todo_density_classify(2), Severity::Ok);
    assert_eq!(todo_density_classify(3), Severity::Medium);
    assert_eq!(todo_density_classify(9), Severity::Medium);
    assert_eq!(todo_density_classify(10), Severity::High);
    assert_eq!(todo_density_classify(100), Severity::High);
}
