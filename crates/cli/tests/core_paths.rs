use heal_cli::core::paths::find_project_root;
use heal_cli::core::HealPaths;

#[test]
fn ensure_creates_all_subdirs() {
    let dir = tempfile::tempdir().unwrap();
    let paths = HealPaths::new(dir.path());
    paths.ensure().unwrap();

    assert!(
        paths.root().join("findings").is_dir(),
        "ensure() must create findings/"
    );
}

#[test]
fn ensure_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let paths = HealPaths::new(dir.path());
    paths.ensure().unwrap();
    paths.ensure().unwrap();
}

fn write_initialised_dot_heal(root: &std::path::Path) {
    let dot_heal = root.join(".heal");
    std::fs::create_dir(&dot_heal).unwrap();
    std::fs::write(dot_heal.join("config.toml"), b"").unwrap();
}

#[test]
fn find_project_root_returns_self_when_dot_heal_at_start() {
    let dir = tempfile::tempdir().unwrap();
    write_initialised_dot_heal(dir.path());

    assert_eq!(find_project_root(dir.path()), dir.path());
}

#[test]
fn find_project_root_walks_up_to_ancestor_with_dot_heal() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_initialised_dot_heal(root);
    let nested = root.join("docs").join("ja");
    std::fs::create_dir_all(&nested).unwrap();

    assert_eq!(find_project_root(&nested), root);
}

#[test]
fn find_project_root_falls_back_to_start_when_no_dot_heal() {
    // Fresh-project case: `heal init` should still materialise
    // `.heal/` at CWD, so the fallback returns the input unchanged.
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a").join("b");
    std::fs::create_dir_all(&nested).unwrap();

    assert_eq!(find_project_root(&nested), nested);
}

#[test]
fn find_project_root_skips_empty_dot_heal_without_config() {
    // A `.heal/` directory with no `config.toml` is the residue of an
    // aborted `heal status` (the command's `paths.ensure()` runs
    // before the config load). Walk past it to the real root above.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_initialised_dot_heal(root);
    let stray = root.join("docs");
    std::fs::create_dir(&stray).unwrap();
    std::fs::create_dir(stray.join(".heal")).unwrap();

    assert_eq!(find_project_root(&stray), root);
}

#[test]
fn find_project_root_ignores_dot_heal_file() {
    // A regular file named `.heal` is not a HEAL project; the walk
    // must keep looking past it (or fall back).
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".heal"), b"not a dir").unwrap();

    assert_eq!(find_project_root(dir.path()), dir.path());
}
