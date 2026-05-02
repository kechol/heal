//! Integration coverage for `ChangeCouplingObserver`: walks tempdir-backed
//! git histories and verifies pair/sum aggregation, the `min_coupling`
//! filter, and the bulk-commit cap.

use std::time::{SystemTime, UNIX_EPOCH};

use heal_cli::observer::change_coupling::{ChangeCouplingObserver, PairDirection};

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

fn observer(min_coupling: u32) -> ChangeCouplingObserver {
    ChangeCouplingObserver {
        enabled: true,
        excluded: Vec::new(),
        since_days: 90,
        min_coupling,
        // Existing tests pre-date the lift filter; turning it off keeps
        // their hand-built repos passing without rebuilding for chance.
        min_lift: 0.0,
        symmetric_threshold: 0.5,
        workspace: None,
    }
}

#[test]
fn empty_when_disabled_or_outside_repo() {
    let dir = tempfile::tempdir().unwrap();
    let report = observer(1).scan(dir.path());
    assert!(report.pairs.is_empty());
    assert_eq!(report.totals.commits_considered, 0);

    let _ = init_repo(dir.path());
    let disabled = ChangeCouplingObserver {
        enabled: false,
        ..observer(1)
    };
    let report = disabled.scan(dir.path());
    assert!(report.pairs.is_empty());
}

#[test]
fn counts_co_occurring_pairs() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();

    // 3 commits touch (a, b, c) → contributes 3 to each of (a,b), (a,c), (b,c)
    for i in 0..3 {
        commit_files(
            &repo,
            &[
                ("a.rs", &format!("a{i}\n")),
                ("b.rs", &format!("b{i}\n")),
                ("c.rs", &format!("c{i}\n")),
            ],
            &format!("abc {i}"),
            now - 100 + i,
        );
    }
    // 2 commits touch only (a, b)
    for i in 0..2 {
        commit_files(
            &repo,
            &[
                ("a.rs", &format!("a-extra{i}\n")),
                ("b.rs", &format!("b-extra{i}\n")),
            ],
            &format!("ab {i}"),
            now - 50 + i,
        );
    }
    // 1 commit touches (a, c)
    commit_files(
        &repo,
        &[("a.rs", "a-final\n"), ("c.rs", "c-final\n")],
        "ac",
        now - 10,
    );

    let report = observer(1).scan(dir.path());
    let lookup = |a: &str, b: &str| {
        report
            .pairs
            .iter()
            .find(|p| p.a.to_string_lossy() == a && p.b.to_string_lossy() == b)
            .map(|p| p.count)
    };
    assert_eq!(lookup("a.rs", "b.rs"), Some(5)); // 3 + 2
    assert_eq!(lookup("a.rs", "c.rs"), Some(4)); // 3 + 1
    assert_eq!(lookup("b.rs", "c.rs"), Some(3)); // 3

    // Pairs sorted by count desc.
    assert_eq!(report.pairs[0].count, 5);
    assert_eq!(report.pairs[1].count, 4);
    assert_eq!(report.pairs[2].count, 3);

    // sum-of-coupling: a participates in (a,b)=5 + (a,c)=4 = 9; b: 5+3=8; c: 4+3=7.
    let sum = |path: &str| {
        report
            .file_sums
            .iter()
            .find(|s| s.path.to_string_lossy() == path)
            .map(|s| s.sum)
    };
    assert_eq!(sum("a.rs"), Some(9));
    assert_eq!(sum("b.rs"), Some(8));
    assert_eq!(sum("c.rs"), Some(7));

    // file_sums sorted by sum desc.
    assert_eq!(report.file_sums[0].path.to_string_lossy(), "a.rs");
}

#[test]
fn min_coupling_filters_low_count_pairs() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();

    // (a,b) co-changes 3 times, (a,c) once.
    for i in 0..3 {
        commit_files(
            &repo,
            &[("a.rs", &format!("a{i}\n")), ("b.rs", &format!("b{i}\n"))],
            &format!("ab {i}"),
            now - 100 + i,
        );
    }
    commit_files(
        &repo,
        &[("a.rs", "a-x\n"), ("c.rs", "c-x\n")],
        "ac",
        now - 10,
    );

    let report = observer(2).scan(dir.path());
    assert_eq!(report.pairs.len(), 1);
    assert_eq!(report.pairs[0].a.to_string_lossy(), "a.rs");
    assert_eq!(report.pairs[0].b.to_string_lossy(), "b.rs");
    assert_eq!(report.pairs[0].count, 3);
}

#[test]
fn bulk_commits_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();

    // 51 files in one commit → exceeds BULK_COMMIT_FILE_LIMIT (50), should be ignored.
    let bodies: Vec<(String, String)> = (0..51)
        .map(|i| (format!("bulk/f{i}.rs"), format!("x{i}\n")))
        .collect();
    let refs: Vec<(&str, &str)> = bodies
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_str()))
        .collect();
    commit_files(&repo, &refs, "bulk", now - 10);

    let report = observer(1).scan(dir.path());
    assert!(report.pairs.is_empty(), "got {:?}", report.pairs);
    assert_eq!(report.totals.commits_considered, 0);
}

#[test]
fn worst_n_pairs_and_files_truncate_in_existing_order() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();

    // Build a 4-file co-change matrix with distinct counts so the ranking is
    // unambiguous: (a,b)=3, (a,c)=2, (b,c)=1, (c,d)=1.
    for i in 0..3 {
        commit_files(
            &repo,
            &[("a.rs", &format!("a{i}\n")), ("b.rs", &format!("b{i}\n"))],
            &format!("ab {i}"),
            now - 100 + i,
        );
    }
    for i in 0..2 {
        commit_files(
            &repo,
            &[("a.rs", &format!("a-x{i}\n")), ("c.rs", &format!("c{i}\n"))],
            &format!("ac {i}"),
            now - 80 + i,
        );
    }
    commit_files(
        &repo,
        &[("b.rs", "b-y\n"), ("c.rs", "c-y\n")],
        "bc",
        now - 60,
    );
    commit_files(
        &repo,
        &[("c.rs", "c-z\n"), ("d.rs", "d-z\n")],
        "cd",
        now - 40,
    );

    let report = observer(1).scan(dir.path());
    assert_eq!(report.pairs.len(), 4);

    let top2 = report.worst_n_pairs(2);
    assert_eq!(top2.len(), 2);
    assert_eq!(top2[0].count, 3);
    assert_eq!(top2[1].count, 2);

    // n exceeding length returns everything available.
    assert_eq!(report.worst_n_pairs(99).len(), 4);

    let top_files = report.worst_n_files(2);
    assert_eq!(top_files.len(), 2);
    assert!(top_files[0].sum >= top_files[1].sum);
}

#[test]
fn excluded_substrings_skip_paths() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();

    commit_files(
        &repo,
        &[
            ("src/a.rs", "1\n"),
            ("vendor/v.rs", "1\n"),
            ("src/b.rs", "1\n"),
        ],
        "init",
        now - 10,
    );

    let observer = ChangeCouplingObserver {
        enabled: true,
        excluded: vec!["vendor".to_string()],
        since_days: 90,
        min_coupling: 1,
        min_lift: 0.0,
        symmetric_threshold: 0.5,
        workspace: None,
    };
    let report = observer.scan(dir.path());
    assert_eq!(report.pairs.len(), 1);
    assert_eq!(report.pairs[0].a.to_string_lossy(), "src/a.rs");
    assert_eq!(report.pairs[0].b.to_string_lossy(), "src/b.rs");
}

/// Two files that always co-edit and never appear alone classify as
/// Symmetric — `P(B|A) = P(A|B) = 1.0`, both above the default 0.5.
#[test]
fn symmetric_pair_when_both_conditional_probs_high() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();
    for i in 0..4 {
        commit_files(
            &repo,
            &[("a.rs", &format!("a{i}\n")), ("b.rs", &format!("b{i}\n"))],
            &format!("ab {i}"),
            now - 100 + i,
        );
    }
    let report = observer(2).scan(dir.path());
    assert_eq!(report.pairs.len(), 1);
    let pair = &report.pairs[0];
    assert_eq!(pair.count, 4);
    assert_eq!(pair.direction, Some(PairDirection::Symmetric));
}

/// One file changes alone often, the other always tags along →
/// classify as `OneWay { from: leader, to: follower }`. Here `core.rs`
/// leads (10 solo edits + 4 with `extras.rs`) and `extras.rs` follows
/// (never alone).
#[test]
fn one_way_pair_picks_leader_by_conditional_probability() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();
    // 4 co-edits with extras.rs.
    for i in 0..4 {
        commit_files(
            &repo,
            &[
                ("core.rs", &format!("c{i}\n")),
                ("extras.rs", &format!("e{i}\n")),
            ],
            &format!("co {i}"),
            now - 200 + i,
        );
    }
    // 10 solo edits to core.rs — extras.rs never appears without core.rs
    // but core.rs ships fine alone.
    for i in 0..10 {
        commit_files(
            &repo,
            &[("core.rs", &format!("solo{i}\n"))],
            &format!("solo {i}"),
            now - 100 + i,
        );
    }
    let report = observer(2).scan(dir.path());
    assert_eq!(report.pairs.len(), 1);
    let pair = &report.pairs[0];
    assert_eq!(pair.count, 4);
    let direction = pair.direction.as_ref().expect("direction populated");
    let PairDirection::OneWay { from, to } = direction else {
        panic!("expected OneWay, got {direction:?}");
    };
    assert_eq!(
        from.to_string_lossy(),
        "core.rs",
        "core.rs is the leader — its solo edits drive extras.rs",
    );
    assert_eq!(to.to_string_lossy(), "extras.rs");
}

/// `symmetric_threshold` is the only knob the user can tune at runtime —
/// confirm raising it past `1.0` (impossible) downgrades every pair to
/// `OneWay` even when the underlying numbers would otherwise classify
/// as `Symmetric`.
#[test]
fn symmetric_threshold_above_one_forces_one_way() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();
    for i in 0..3 {
        commit_files(
            &repo,
            &[("a.rs", &format!("a{i}\n")), ("b.rs", &format!("b{i}\n"))],
            &format!("ab {i}"),
            now - 100 + i,
        );
    }
    let strict = ChangeCouplingObserver {
        enabled: true,
        excluded: Vec::new(),
        since_days: 90,
        min_coupling: 2,
        min_lift: 0.0,
        symmetric_threshold: 1.5,
        workspace: None,
    };
    let report = strict.scan(dir.path());
    assert_eq!(report.pairs.len(), 1);
    assert!(matches!(
        report.pairs[0].direction,
        Some(PairDirection::OneWay { .. })
    ));
}
