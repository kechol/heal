//! Integration coverage for `DuplicationObserver`: tempdir fixtures
//! exercising tokenization, bucketing, and greedy block extension.

use heal_cli::observer::duplication::DuplicationObserver;

mod common;
use common::write;

fn observer(min_tokens: u32) -> DuplicationObserver {
    DuplicationObserver {
        enabled: true,
        excluded: Vec::new(),
        min_tokens,
        workspace: None,
    }
}

/// A 60-token-ish Rust function. Repeated literally in two files to make
/// the duplicate easy to assert against without depending on grammar
/// internals.
fn duplicate_block() -> &'static str {
    "fn calc(a: i32, b: i32, c: i32, d: i32) -> i32 {\n    \
     let x = a + b;\n    \
     let y = c + d;\n    \
     let z = x * y;\n    \
     let w = z + x + y;\n    \
     let q = w * 2;\n    \
     let r = q + 1;\n    \
     let s = r + q;\n    \
     s + w + z + y + x\n}\n"
}

#[test]
fn empty_when_disabled() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/a.rs", duplicate_block());
    write(dir.path(), "src/b.rs", duplicate_block());
    let report = DuplicationObserver {
        enabled: false,
        ..observer(20)
    }
    .scan(dir.path());
    assert!(report.blocks.is_empty());
    assert_eq!(report.min_tokens, 20);
}

#[cfg(feature = "lang-rust")]
#[test]
fn detects_cross_file_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/a.rs", duplicate_block());
    write(dir.path(), "src/b.rs", duplicate_block());

    let report = observer(20).scan(dir.path());
    assert_eq!(
        report.blocks.len(),
        1,
        "expected single block, got {:?}",
        report.blocks
    );
    let block = &report.blocks[0];
    assert_eq!(block.locations.len(), 2);
    assert!(block.token_count >= 20);
    let paths: Vec<String> = block
        .locations
        .iter()
        .map(|l| l.path.to_string_lossy().into_owned())
        .collect();
    assert!(paths.contains(&"src/a.rs".to_string()), "got {paths:?}");
    assert!(paths.contains(&"src/b.rs".to_string()), "got {paths:?}");
    assert_eq!(report.totals.duplicate_blocks, 1);
    assert_eq!(report.totals.files_affected, 2);
}

#[test]
fn skips_when_below_min_tokens_threshold() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/a.rs", duplicate_block());
    write(dir.path(), "src/b.rs", duplicate_block());

    // 1000 tokens is far above what the duplicate body produces, so no
    // window survives the filter.
    let report = observer(1000).scan(dir.path());
    assert!(report.blocks.is_empty());
}

#[test]
fn unique_files_yield_no_blocks() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/a.rs", "fn one(a: i32) -> i32 { a + 1 }\n");
    write(
        dir.path(),
        "src/b.rs",
        "struct Foo { bar: u32, baz: String }\n",
    );

    let report = observer(10).scan(dir.path());
    assert!(report.blocks.is_empty());
}

#[test]
fn excluded_substrings_skip_files() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/a.rs", duplicate_block());
    write(dir.path(), "vendor/b.rs", duplicate_block());

    let observer = DuplicationObserver {
        enabled: true,
        excluded: vec!["vendor".to_string()],
        min_tokens: 20,
        workspace: None,
    };
    let report = observer.scan(dir.path());
    // Only one file remains after exclusion → nothing to compare against.
    assert!(report.blocks.is_empty());
}

#[cfg(feature = "lang-rust")]
#[test]
fn worst_n_blocks_truncates_in_existing_order() {
    let dir = tempfile::tempdir().unwrap();
    // First duplicate: full block (largest).
    write(dir.path(), "src/a.rs", duplicate_block());
    write(dir.path(), "src/b.rs", duplicate_block());
    // Second duplicate: shorter shared body (smaller).
    let small = "fn helper(x: i32, y: i32) -> i32 {\n    \
                 let a = x + y;\n    \
                 let b = a * 2;\n    \
                 let c = b + a;\n    \
                 c + a + b + x + y\n}\n";
    write(dir.path(), "src/c.rs", small);
    write(dir.path(), "src/d.rs", small);

    let report = observer(15).scan(dir.path());
    assert!(
        report.blocks.len() >= 2,
        "expected ≥2 blocks, got {:?}",
        report.blocks,
    );

    // worst_n_blocks(1) returns the largest by token_count.
    let top1 = report.worst_n_blocks(1);
    assert_eq!(top1.len(), 1);
    assert_eq!(top1[0].token_count, report.blocks[0].token_count);

    // n exceeding length returns everything available.
    assert_eq!(report.worst_n_blocks(99).len(), report.blocks.len());
}

#[cfg(feature = "lang-typescript")]
#[test]
fn typescript_duplicates_are_detected() {
    let dir = tempfile::tempdir().unwrap();
    let body = "function calc(a: number, b: number, c: number): number {\n  \
                const x = a + b;\n  \
                const y = b + c;\n  \
                const z = x * y;\n  \
                const w = z + x;\n  \
                const q = w + y;\n  \
                return q + z + x + y + w;\n\
                }\n";
    write(dir.path(), "src/a.ts", body);
    write(dir.path(), "src/b.ts", body);

    let report = observer(20).scan(dir.path());
    assert_eq!(report.blocks.len(), 1);
    let paths: Vec<String> = report.blocks[0]
        .locations
        .iter()
        .map(|l| l.path.to_string_lossy().into_owned())
        .collect();
    assert!(paths.contains(&"src/a.ts".to_string()));
    assert!(paths.contains(&"src/b.ts".to_string()));
}
