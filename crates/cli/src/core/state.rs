//! `.heal/state.json` — runtime state (currently `open_proposals`).
//!
//! v0.2 retired the `last_fired` cool-down map together with the
//! `SessionStart` nudge. The struct is kept around so v0.2's
//! `policy.action = execute` path (TODO) has a place to land
//! `open_proposals`; full state.json removal lives behind that work
//! (TODO §state.json 撤去).

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::error::{Error, Result};

/// Runtime state. Forward-compatible by design — unknown fields are tolerated
/// so an older binary never fails to read a state file written by a newer one.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct State {
    #[serde(default)]
    pub open_proposals: BTreeMap<String, OpenProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

    /// Atomic write: serialize, write to a sibling temp file, then rename.
    /// Avoids leaving a half-written `state.json` behind after SIGINT — a
    /// truncated file would otherwise make every subsequent `State::load`
    /// invocation hard-error on parse until the user deletes it.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        let body = serde_json::to_string_pretty(self).expect("State serialization is infallible");
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
}
