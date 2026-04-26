//! Helpers shared across the observer crate's integration tests.
//!
//! Cargo treats each file directly under `tests/` as its own integration
//! binary; modules under `tests/common/` are pulled in via `mod common;` in
//! each binary that needs them and don't compile as a standalone test.

use std::fs;
use std::path::Path;

/// Write `body` to `root.join(rel)`, creating intermediate directories.
/// Panics on I/O failure — appropriate inside `#[test]` fixtures.
pub fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}
