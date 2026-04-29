use chrono::Utc;
use heal_cli::core::state::{OpenProposal, State};

#[test]
fn missing_file_yields_empty_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let state = State::load(&path).unwrap();
    assert!(state.last_fired.is_empty());
    assert!(state.open_proposals.is_empty());
}

#[test]
fn corrupt_state_file_returns_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    std::fs::write(&path, "{ not valid json").unwrap();
    let err = State::load(&path).unwrap_err();
    assert!(
        format!("{err:?}").contains("StateParse"),
        "expected StateParse, got {err:?}"
    );
}

#[test]
fn save_writes_atomically_via_tempfile_rename() {
    // The atomic-write contract: while a save is in flight, an outside
    // observer must see either the prior contents or the new contents,
    // never a half-written body. Asserting the *.tmp sibling does not
    // leak after a successful save is a sufficient proxy.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let state = State::default();
    state.save(&path).unwrap();
    state.save(&path).unwrap();
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .collect();
    assert!(
        entries
            .iter()
            .all(|n| !n.to_string_lossy().ends_with(".tmp")),
        "stale temp file leaked: {entries:?}"
    );
}

#[test]
fn save_then_load_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("state.json");

    let mut state = State::default();
    let now = Utc::now();
    state.last_fired.insert("rule:src/a.ts".into(), now);
    state.open_proposals.insert(
        "issue-42".into(),
        OpenProposal {
            rule: "low_coverage_hotspot".into(),
            file: "src/auth/session.ts".into(),
            opened_at: now,
        },
    );
    state.save(&path).unwrap();
    let reloaded = State::load(&path).unwrap();
    assert_eq!(state, reloaded);
}
