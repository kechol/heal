use heal_cli::core::HealPaths;

#[test]
fn ensure_creates_all_subdirs() {
    let dir = tempfile::tempdir().unwrap();
    let paths = HealPaths::new(dir.path());
    paths.ensure().unwrap();

    assert!(
        paths.root().join("checks").is_dir(),
        "ensure() must create checks/"
    );
}

#[test]
fn ensure_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let paths = HealPaths::new(dir.path());
    paths.ensure().unwrap();
    paths.ensure().unwrap();
}
