//! `.heal/state.json` — `last_fired` tracking and `open_proposals`.
//!
//! Kept intentionally small in v0.1; richer trigger/proposal lifecycle lives
//! behind v0.2 once `policy.action = execute` lands.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct State {
    #[serde(default)]
    pub last_fired: BTreeMap<String, DateTime<Utc>>,
    #[serde(default)]
    pub open_proposals: BTreeMap<String, OpenProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OpenProposal {
    pub rule: String,
    pub file: String,
    pub opened_at: DateTime<Utc>,
}

impl State {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str(&raw).map_err(|source| Error::StateParse {
                path: path.to_path_buf(),
                source,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Io {
                path: path.to_path_buf(),
                source: e,
            }),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        let body = serde_json::to_string_pretty(self).expect("State serialization is infallible");
        std::fs::write(path, body).map_err(|e| Error::Io {
            path: path.to_path_buf(),
            source: e,
        })
    }
}
