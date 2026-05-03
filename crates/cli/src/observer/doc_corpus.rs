//! Pre-read doc bodies shared across the `[features.docs]` Layer B
//! observers and the duplication Markdown pass. Reading each Layer B
//! file once in `observers::run_all` and threading the bodies in
//! avoids the 4× I/O the original implementation paid (one read per
//! observer).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// One Layer B doc, already read off disk. Owned `String` so consumers
/// can borrow `&str` slices for line / link / token scanning.
#[derive(Debug, Clone)]
pub struct DocBody {
    pub path: PathBuf,
    pub body: String,
}

impl DocBody {
    #[must_use]
    pub fn new(path: PathBuf, body: String) -> Self {
        Self { path, body }
    }
}

/// Read every unique entry in `paths` once, relative to `root`.
/// Missing or unreadable files are silently dropped — Layer B
/// observers tolerate that the same way they tolerated the per-file
/// `fs::read_to_string` returning `Err` previously. Result preserves
/// input order with later duplicates removed.
#[must_use]
pub fn read_doc_bodies(root: &Path, paths: &[PathBuf]) -> Vec<DocBody> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut out: Vec<DocBody> = Vec::with_capacity(paths.len());
    for path in paths {
        if !seen.insert(path.clone()) {
            continue;
        }
        let abs = root.join(path);
        let Ok(body) = std::fs::read_to_string(&abs) else {
            continue;
        };
        out.push(DocBody::new(path.clone(), body));
    }
    out
}

/// Filter a corpus to entries whose path is in `keep`. Cheap when
/// `keep` is small (Layer B observers often want a subset of the
/// shared corpus). Returned `DocBody`s are clones.
#[must_use]
pub fn select(corpus: &[DocBody], keep: &[PathBuf]) -> Vec<DocBody> {
    let want: HashSet<&Path> = keep.iter().map(PathBuf::as_path).collect();
    corpus
        .iter()
        .filter(|d| want.contains(d.path.as_path()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write(root: &Path, rel: &str, body: &str) {
        let abs = root.join(rel);
        if let Some(p) = abs.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(abs, body).unwrap();
    }

    #[test]
    fn read_doc_bodies_dedupes_and_skips_missing() {
        let dir = tempdir().unwrap();
        write(dir.path(), "a.md", "alpha");
        write(dir.path(), "b.md", "bravo");
        // missing.md is intentionally absent.
        let paths = vec![
            PathBuf::from("a.md"),
            PathBuf::from("missing.md"),
            PathBuf::from("b.md"),
            PathBuf::from("a.md"), // duplicate dropped
        ];
        let corpus = read_doc_bodies(dir.path(), &paths);
        let names: Vec<&str> = corpus.iter().map(|d| d.path.to_str().unwrap()).collect();
        assert_eq!(names, vec!["a.md", "b.md"]);
        assert_eq!(corpus[0].body, "alpha");
        assert_eq!(corpus[1].body, "bravo");
    }

    #[test]
    fn select_filters_to_requested_subset() {
        let corpus = vec![
            DocBody::new(PathBuf::from("a.md"), "x".into()),
            DocBody::new(PathBuf::from("b.md"), "y".into()),
            DocBody::new(PathBuf::from("c.md"), "z".into()),
        ];
        let keep = vec![PathBuf::from("a.md"), PathBuf::from("c.md")];
        let got = select(&corpus, &keep);
        let names: Vec<&str> = got.iter().map(|d| d.path.to_str().unwrap()).collect();
        assert_eq!(names, vec!["a.md", "c.md"]);
    }
}
