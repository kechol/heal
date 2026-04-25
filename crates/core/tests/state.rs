use chrono::Utc;
use heal_core::state::{OpenProposal, State};

#[test]
fn missing_file_yields_empty_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let state = State::load(&path).unwrap();
    assert!(state.last_fired.is_empty());
    assert!(state.open_proposals.is_empty());
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
