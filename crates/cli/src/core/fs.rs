//! Small filesystem helpers shared across `core::*` writers.

use std::path::Path;

use crate::core::error::{Error, Result};

/// Atomic write: create parent directories, write `body` to a sibling
/// `<filename>.tmp` file, then `rename` it into `path`. A SIGINT mid-
/// write therefore leaves either the previous file untouched or the new
/// file fully written — never a half-written stub that breaks
/// every subsequent reader on parse.
pub fn atomic_write(path: &Path, body: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::Io {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }
    let tmp = match path.file_name() {
        Some(name) => {
            let mut t = name.to_os_string();
            t.push(".tmp");
            path.with_file_name(t)
        }
        None => path.with_extension("tmp"),
    };
    std::fs::write(&tmp, body).map_err(|e| Error::Io {
        path: tmp.clone(),
        source: e,
    })?;
    std::fs::rename(&tmp, path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })
}
