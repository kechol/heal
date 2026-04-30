use heal_cli::core::HealPaths;

#[test]
fn ensure_creates_all_subdirs() {
    let dir = tempfile::tempdir().unwrap();
    let paths = HealPaths::new(dir.path());
    paths.ensure().unwrap();

    for sub in ["snapshots", "logs", "docs", "reports"] {
        assert!(paths.root().join(sub).is_dir(), "missing {sub}");
    }
}

#[test]
fn ensure_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let paths = HealPaths::new(dir.path());
    paths.ensure().unwrap();
    paths.ensure().unwrap();
}
