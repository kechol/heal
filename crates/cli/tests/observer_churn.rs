//! Integration coverage for `ChurnObserver`: tempdir-backed git fixtures
//! exercising the revwalk + per-file accumulation pipeline.

use std::time::{SystemTime, UNIX_EPOCH};

use heal_cli::observer::churn::ChurnObserver;

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

fn enabled_observer() -> ChurnObserver {
    ChurnObserver {
        enabled: true,
        excluded: Vec::new(),
        since_days: 90,
        workspace: None,
    }
}

#[test]
fn returns_empty_report_when_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    commit_files(&repo, &[("a.txt", "x\n")], "init", now_secs());

    let observer = ChurnObserver {
        enabled: false,
        ..enabled_observer()
    };
    let report = observer.scan(dir.path());
    assert!(report.files.is_empty());
    assert_eq!(report.totals.commits, 0);
    assert_eq!(report.since_days, 90);
}

#[test]
fn returns_empty_report_outside_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    let report = enabled_observer().scan(dir.path());
    assert!(report.files.is_empty());
    assert_eq!(report.totals.files, 0);
}

#[test]
fn counts_commits_and_lines_per_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();

    commit_files(&repo, &[("src/a.rs", "fn a() {}\n")], "c1", now - 100);
    commit_files(
        &repo,
        &[("src/a.rs", "fn a() {}\nfn aa() {}\n")],
        "c2",
        now - 80,
    );
    commit_files(
        &repo,
        &[("src/a.rs", "fn a() {}\nfn aa() {}\nfn aaa() {}\n")],
        "c3",
        now - 60,
    );
    commit_files(&repo, &[("src/b.rs", "fn b() {}\n")], "c4", now - 40);

    let report = enabled_observer().scan(dir.path());
    assert_eq!(report.totals.commits, 4);

    // worst_n by commits — a.rs (3) > b.rs (1).
    let top = report.worst_n(2);
    assert_eq!(top[0].path.to_string_lossy(), "src/a.rs");
    assert_eq!(top[0].commits, 3);
    assert_eq!(top[1].path.to_string_lossy(), "src/b.rs");
    assert_eq!(top[1].commits, 1);

    // a.rs additions: 1 (c1) + 1 (c2) + 1 (c3) = 3 lines added across history.
    assert_eq!(top[0].lines_added, 3);
    assert_eq!(top[0].lines_deleted, 0);
}

#[test]
fn since_days_excludes_old_commits() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();
    let day = 86_400;

    // 200 days ago (outside window) and 10 days ago (inside window).
    commit_files(&repo, &[("old.rs", "1\n")], "old", now - 200 * day);
    commit_files(&repo, &[("new.rs", "1\n")], "new", now - 10 * day);

    let report = ChurnObserver {
        enabled: true,
        excluded: Vec::new(),
        since_days: 90,
        workspace: None,
    }
    .scan(dir.path());

    let paths: Vec<String> = report
        .files
        .iter()
        .map(|f| f.path.to_string_lossy().into_owned())
        .collect();
    assert!(paths.contains(&"new.rs".to_string()), "got {paths:?}");
    assert!(!paths.contains(&"old.rs".to_string()), "got {paths:?}");
    assert_eq!(report.totals.commits, 1);
}

#[test]
fn excluded_substrings_skip_paths() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();

    commit_files(
        &repo,
        &[("src/keep.rs", "1\n"), ("vendor/skip.rs", "1\n")],
        "init",
        now - 10,
    );

    let observer = ChurnObserver {
        enabled: true,
        excluded: vec!["vendor".to_string()],
        since_days: 90,
        workspace: None,
    };
    let report = observer.scan(dir.path());
    let paths: Vec<String> = report
        .files
        .iter()
        .map(|f| f.path.to_string_lossy().into_owned())
        .collect();
    assert_eq!(paths, vec!["src/keep.rs".to_string()]);
}

#[test]
fn workspace_scope_filters_files_and_recomputes_commits() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path());
    let now = now_secs();

    // Three commits: two touch packages/web only, one touches packages/api only.
    commit_files(
        &repo,
        &[("packages/web/a.ts", "1\n")],
        "web only 1",
        now - 30,
    );
    commit_files(
        &repo,
        &[("packages/web/b.ts", "1\n")],
        "web only 2",
        now - 20,
    );
    commit_files(&repo, &[("packages/api/c.ts", "1\n")], "api only", now - 10);

    let report = ChurnObserver {
        enabled: true,
        excluded: Vec::new(),
        since_days: 90,
        workspace: Some(std::path::PathBuf::from("packages/web")),
    }
    .scan(dir.path());

    // Only the two web files survive; api commit drops entirely.
    let paths: Vec<String> = report
        .files
        .iter()
        .map(|f| f.path.to_string_lossy().into_owned())
        .collect();
    assert_eq!(paths.len(), 2);
    assert!(paths.iter().all(|p| p.starts_with("packages/web/")));
    // commits_in_workspace recount: 2, not 3.
    assert_eq!(report.totals.commits, 2);
}
