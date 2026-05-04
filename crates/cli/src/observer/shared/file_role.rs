//! File-role classification by path conventions: source / test / doc /
//! lockfile / generated. Used by `change_coupling` to tag pair classes
//! and by `Feature::lower` to set `Finding.is_test_file` against the
//! `[features.test].test_paths` matcher.
//!
//! All checks are convention-based (suffix / dirname). They are
//! deliberately path-only — content sniffing belongs elsewhere — so the
//! same call answers identically for a non-existent path.

use std::path::Path;

/// Coarse classification of a single file path. The order in
/// [`file_role`] is significant: lockfile / generated win over test /
/// doc, because a generated `*.test.ts.snap` is a build artefact, not a
/// test source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRole {
    Source,
    Test,
    Doc,
    Lockfile,
    Generated,
}

/// Classify `path` by convention. `primary_lang` gates language-specific
/// generated-directory markers (`target/` for Rust/Scala, `*.egg-info/`
/// for Python) that would be ambiguous in other ecosystems.
#[must_use]
pub fn file_role(path: &Path, primary_lang: Option<&str>) -> FileRole {
    let path_str = path.to_string_lossy();
    let basename = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default();

    if is_lockfile(basename, primary_lang) {
        return FileRole::Lockfile;
    }
    if is_generated(&path_str, basename, primary_lang) {
        return FileRole::Generated;
    }
    if is_test(&path_str, basename) {
        return FileRole::Test;
    }
    if is_doc(&path_str, basename) {
        return FileRole::Doc;
    }
    FileRole::Source
}

/// True when `path`'s convention says it's a test source (suffix like
/// `.test.ts`, basename like `test_*.py`, or under `tests/` /
/// `__tests__/` / `spec/` / `test/`). Independent of language detection.
#[must_use]
pub fn is_test_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    let basename = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default();
    is_test(&path_str, basename)
}

// Lockfile / generated / test / doc filenames are convention, always
// lowercase in practice. The lint about case-sensitive extension
// comparison would force `eq_ignore_ascii_case` rituals that add no
// signal.
#[allow(clippy::case_sensitive_file_extension_comparisons)]
pub(crate) fn is_lockfile(basename: &str, _primary_lang: Option<&str>) -> bool {
    // Generic suffixes (`*.lock`, `*.lockb`, `go.sum`) catch most
    // ecosystems. Well-known basenames are matched unconditionally
    // because monorepos commonly mix languages — a Rust workspace's
    // `docs/` may still carry a `package-lock.json`, and the primary
    // language detection is project-wide.
    if basename.ends_with(".lock") || basename.ends_with(".lockb") || basename == "go.sum" {
        return true;
    }
    matches!(
        basename,
        "package-lock.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "bun.lock"
            | "bun.lockb"
            | "poetry.lock"
            | "Pipfile.lock"
            | "uv.lock"
            | "composer.lock"
            | "Gemfile.lock"
    )
}

#[allow(clippy::case_sensitive_file_extension_comparisons)]
pub(crate) fn is_generated(path_str: &str, basename: &str, primary_lang: Option<&str>) -> bool {
    // Cross-language directory markers — covers most tooling output.
    // Match both "<root>/dir/" (sub-path) and "dir/" at the start of the
    // path (no leading slash).
    const COMMON_DIRS: &[&str] = &[
        "dist/",
        "build/",
        "__generated__/",
        "generated/",
        "__pycache__/",
        "node_modules/",
        "vendor/",
    ];
    if dir_marker_matches(path_str, COMMON_DIRS) {
        return true;
    }
    // Generated artifacts that ship next to source instead of in dist/.
    if basename.ends_with(".min.js")
        || basename.ends_with(".min.css")
        || basename.contains(".bundle.")
        || basename.ends_with(".snap")
    {
        return true;
    }
    match primary_lang {
        // `target/` is a Cargo / sbt build dir. Gated by primary
        // language because some non-Rust / non-Scala projects use
        // `target/` for unrelated meaning (e.g. front-end build
        // pipelines targeting a `target/` output).
        Some("rust" | "scala") => dir_marker_matches(path_str, &["target/"]),
        Some("python") => path_str.contains(".egg-info/"),
        _ => false,
    }
}

/// True iff any of `dirs` (each a `name/` form, no leading slash)
/// appears as a path component — either at the start of the string or
/// preceded by `/`.
pub(crate) fn dir_marker_matches(path_str: &str, dirs: &[&str]) -> bool {
    dirs.iter()
        .any(|d| path_str.starts_with(d) || path_str.contains(&format!("/{d}")))
}

#[allow(clippy::case_sensitive_file_extension_comparisons)]
pub(crate) fn is_test(path_str: &str, basename: &str) -> bool {
    // Suffix-based: `foo.test.ts`, `foo_test.go`, `test_foo.py`, `foo.spec.ts`.
    if basename.contains(".test.")
        || basename.contains(".spec.")
        || basename.starts_with("test_")
        || basename.ends_with("_test.go")
        || basename.ends_with("_test.rs")
        || basename.ends_with("_test.py")
    {
        return true;
    }
    dir_marker_matches(path_str, &["tests/", "__tests__/", "spec/", "test/"])
}

#[allow(clippy::case_sensitive_file_extension_comparisons)]
pub(crate) fn is_doc(path_str: &str, basename: &str) -> bool {
    if basename.ends_with(".md") || basename.ends_with(".mdx") || basename.ends_with(".rst") {
        return true;
    }
    dir_marker_matches(path_str, &["docs/"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn classifies_test_paths() {
        assert!(is_test_path(&PathBuf::from("tests/foo.rs")));
        assert!(is_test_path(&PathBuf::from("src/foo_test.go")));
        assert!(is_test_path(&PathBuf::from(
            "packages/web/src/Button.test.tsx"
        )));
        assert!(is_test_path(&PathBuf::from("__tests__/Button.tsx")));
        assert!(is_test_path(&PathBuf::from("api/test_users.py")));
        assert!(!is_test_path(&PathBuf::from("src/lib.rs")));
        assert!(!is_test_path(&PathBuf::from("docs/cli.md")));
    }

    #[test]
    fn lockfile_and_generated_take_priority_over_test() {
        assert_eq!(
            file_role(&PathBuf::from("tests/__pycache__/x.py"), Some("python")),
            FileRole::Generated
        );
        assert_eq!(
            file_role(&PathBuf::from("tests/Cargo.lock"), Some("rust")),
            FileRole::Lockfile
        );
    }

    #[test]
    fn doc_and_source_classification() {
        assert_eq!(
            file_role(&PathBuf::from("docs/cli.md"), Some("rust")),
            FileRole::Doc
        );
        assert_eq!(
            file_role(&PathBuf::from("README.md"), Some("rust")),
            FileRole::Doc
        );
        assert_eq!(
            file_role(&PathBuf::from("crates/cli/src/lib.rs"), Some("rust")),
            FileRole::Source
        );
    }
}
