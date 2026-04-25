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

    #[error("invalid history record at {path}: {source}")]
    HistoryParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("invalid state json at {path}: {source}")]
    StateParse {
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

pub type Result<T> = std::result::Result<T, Error>;
