use heal_core::HealPaths;

#[test]
fn ensure_creates_all_subdirs() {
    let dir = tempfile::tempdir().unwrap();
    let paths = HealPaths::new(dir.path());
    paths.ensure().unwrap();

    for sub in ["runtime", "snapshots", "logs", "docs", "reports"] {
        assert!(paths.root().join(sub).is_dir(), "missing {sub}");
    }
}

#[test]
fn state_lives_under_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let paths = HealPaths::new(dir.path());
    assert_eq!(paths.state(), paths.runtime_dir().join("state.json"));
}

#[test]
fn ensure_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let paths = HealPaths::new(dir.path());
    paths.ensure().unwrap();
    paths.ensure().unwrap();
}
