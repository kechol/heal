use std::path::PathBuf;

use heal_cli::core::doc_pairs::{DocPair, DocPairsFile, PairSource, DOC_PAIRS_VERSION};

fn write(project: &std::path::Path, rel: &str, body: &str) {
    let abs = project.join(rel);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&abs, body).unwrap();
}

#[test]
fn read_returns_none_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let got = DocPairsFile::read(dir.path(), ".heal/doc_pairs.json").unwrap();
    assert!(got.is_none());
}

#[test]
fn read_round_trips_full_entry() {
    let dir = tempfile::tempdir().unwrap();
    let body = r#"{
        "version": 1,
        "pairs": [
            {
                "doc": "docs/cli.md",
                "srcs": ["src/cli.rs"],
                "confidence": 0.85,
                "source": "mention"
            }
        ]
    }"#;
    write(dir.path(), ".heal/doc_pairs.json", body);
    let parsed = DocPairsFile::read(dir.path(), ".heal/doc_pairs.json")
        .unwrap()
        .expect("file present");
    assert_eq!(parsed.version, DOC_PAIRS_VERSION);
    assert_eq!(parsed.pairs.len(), 1);
    assert_eq!(parsed.pairs[0].doc, "docs/cli.md");
    assert_eq!(parsed.pairs[0].srcs, vec!["src/cli.rs".to_string()]);
    assert_eq!(parsed.pairs[0].confidence, Some(0.85));
    assert_eq!(parsed.pairs[0].source, Some(PairSource::Mention));
}

#[test]
fn read_treats_unknown_version_as_absent() {
    let dir = tempfile::tempdir().unwrap();
    // version 999 ⇒ schema mismatch ⇒ silently invalidate so the user
    // can rerun the generator without a hard parse error.
    let body = r#"{"version": 999, "pairs": []}"#;
    write(dir.path(), ".heal/doc_pairs.json", body);
    let got = DocPairsFile::read(dir.path(), ".heal/doc_pairs.json").unwrap();
    assert!(got.is_none());
}

#[test]
fn read_rejects_malformed_json() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".heal/doc_pairs.json", "{ not json }");
    let err = DocPairsFile::read(dir.path(), ".heal/doc_pairs.json").unwrap_err();
    let s = format!("{err}");
    assert!(s.contains("invalid cache record"), "got: {s}");
}

#[test]
fn read_rejects_unknown_fields() {
    let dir = tempfile::tempdir().unwrap();
    let body = r#"{
        "version": 1,
        "pairs": [],
        "bogus_top_level": true
    }"#;
    write(dir.path(), ".heal/doc_pairs.json", body);
    assert!(DocPairsFile::read(dir.path(), ".heal/doc_pairs.json").is_err());
}

#[test]
fn integrity_check_clean_when_all_paths_exist() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/cli.md", "# CLI\n");
    write(dir.path(), "src/cli.rs", "fn main() {}\n");
    let file = DocPairsFile {
        version: DOC_PAIRS_VERSION,
        pairs: vec![DocPair {
            doc: "docs/cli.md".into(),
            srcs: vec!["src/cli.rs".into()],
            confidence: None,
            source: Some(PairSource::Manual),
        }],
    };
    assert!(file.integrity_check(dir.path()).is_empty());
}

#[test]
fn integrity_check_reports_missing_paths() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/cli.md", "# CLI\n");
    // src/cli.rs intentionally absent.
    let file = DocPairsFile {
        version: DOC_PAIRS_VERSION,
        pairs: vec![DocPair {
            doc: "docs/cli.md".into(),
            srcs: vec!["src/cli.rs".into()],
            confidence: None,
            source: None,
        }],
    };
    let warnings = file.integrity_check(dir.path());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].pair_index, 0);
    assert_eq!(warnings[0].missing_path, PathBuf::from("src/cli.rs"));
}

#[test]
fn live_pairs_filters_dangling_entries() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/a.md", "");
    write(dir.path(), "src/a.rs", "");
    // Pair 1 healthy; pair 2 has a missing src; pair 3 missing doc.
    let file = DocPairsFile {
        version: DOC_PAIRS_VERSION,
        pairs: vec![
            DocPair {
                doc: "docs/a.md".into(),
                srcs: vec!["src/a.rs".into()],
                confidence: None,
                source: None,
            },
            DocPair {
                doc: "docs/a.md".into(),
                srcs: vec!["src/missing.rs".into()],
                confidence: None,
                source: None,
            },
            DocPair {
                doc: "docs/missing.md".into(),
                srcs: vec!["src/a.rs".into()],
                confidence: None,
                source: None,
            },
        ],
    };
    let live = file.live_pairs(dir.path());
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].doc, "docs/a.md");
}

#[test]
fn pair_source_serializes_snake_case() {
    let pair = DocPair {
        doc: "x.md".into(),
        srcs: vec!["x.rs".into()],
        confidence: None,
        source: Some(PairSource::Llm),
    };
    let s = serde_json::to_string(&pair).unwrap();
    assert!(s.contains("\"llm\""), "got: {s}");
}
