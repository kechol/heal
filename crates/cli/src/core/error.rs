use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config not found at {0}; run `heal init` first")]
    ConfigMissing(PathBuf),

    #[error("invalid config at {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("invalid config at {path}: {message}")]
    ConfigInvalid { path: PathBuf, message: String },

    #[error("invalid cache record at {path}: {source}")]
    CacheParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl Error {
    /// True for an [`Error::Io`] wrapping `NotFound` — the "file simply
    /// isn't there" case that probing callers (e.g. the coverage
    /// observer's `lcov_paths` loop) treat as a silent skip, as opposed
    /// to a permission or encoding failure worth surfacing.
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            Self::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound
        )
    }
}

pub type Result<T> = std::result::Result<T, Error>;
