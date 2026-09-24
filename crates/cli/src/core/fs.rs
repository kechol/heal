//! Small filesystem helpers shared across `core::*` writers.

use std::io::Write;
use std::path::Path;

use crate::core::error::{Error, Result};

/// Atomic write: create parent directories, write `body` to a sibling
/// uniquely named temporary file, then rename it into `path`. A SIGINT mid-
/// write therefore leaves either the previous file untouched or the new
/// file fully written — never a half-written stub that breaks
/// every subsequent reader on parse.
pub fn atomic_write(path: &Path, body: &[u8]) -> Result<()> {
    // Match std::fs::write's creation mode; tempfile still applies the umask.
    atomic_write_mode(path, body, 0o666)
}

/// [`atomic_write`] with an explicit Unix creation mode (ignored elsewhere),
/// for files that must never be readable by others, such as a stored key
/// (`0o600`).
pub fn atomic_write_mode(path: &Path, body: &[u8], mode: u32) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| Error::Io {
        path: parent.to_path_buf(),
        source: e,
    })?;
    // Independent writers must never truncate or rename each other's staging file.
    let mut builder = tempfile::Builder::new();
    builder.prefix(".heal-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut tmp = builder.tempfile_in(parent).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    tmp.write_all(body).map_err(|e| Error::Io {
        path: tmp.path().to_path_buf(),
        source: e,
    })?;
    tmp.persist(path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e.error,
    })?;
    Ok(())
}

/// True when `dir` exists, is readable, and contains no entries.
/// Returns `false` for missing or non-directory paths.
#[must_use]
pub fn dir_is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut it| it.next().is_none())
}

/// `rmdir` the directory if it's empty; ignore the call otherwise.
/// Idempotent — useful for cleaning up parent dirs after removing
/// the only file inside.
pub fn remove_dir_if_empty(dir: &Path) -> std::io::Result<()> {
    if !dir_is_empty(dir) {
        return Ok(());
    }
    match std::fs::remove_dir(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_existing_file_without_touching_tmp_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/state.json");
        atomic_write(&path, b"old").unwrap();
        let sibling = path.with_file_name("state.json.tmp");
        std::fs::write(&sibling, b"another writer").unwrap();

        atomic_write(&path, b"new").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(std::fs::read(&sibling).unwrap(), b"another writer");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            2
        );
    }

    #[test]
    fn concurrent_atomic_writes_keep_one_complete_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for byte in 0..8_u8 {
                let path = &path;
                let barrier = &barrier;
                scope.spawn(move || {
                    let body = vec![byte; 64 * 1024];
                    barrier.wait();
                    atomic_write(path, &body).unwrap();
                });
            }
        });

        let body = std::fs::read(&path).unwrap();
        assert_eq!(body.len(), 64 * 1024);
        assert!(body.iter().all(|byte| *byte == body[0]));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn atomic_write_cleans_up_staging_file_when_persist_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("directory");
        std::fs::create_dir(&path).unwrap();

        assert!(atomic_write(&path, b"data").is_err());

        assert!(path.is_dir());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
