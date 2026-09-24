//! `.heal/concepts.toml` — the project's concept vocabulary.
//!
//! A short list of the ideas the codebase is built from, each with a
//! one-line definition. The `concept` and `doc_concept`
//! semantic tasks classify every function and doc section into one of
//! them; Jev cannot invent names, so the vocabulary is written by the
//! agent (`/heal:setup`) and reviewed by the team. Tracked in git
//! like `config.toml`: it is part of the team contract (`scope.md` R6).
//!
//! ```toml
//! [[concept]]
//! id = "calibration"
//! description = "Derives codebase-relative thresholds from the project's own metric distribution."
//! ```

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::error::{Error, Result};

/// Label every classification can fall back to. Added automatically so a
/// function that fits no concept is not forced into the nearest one.
pub const OTHER: &str = "other";
const OTHER_DESCRIPTION: &str = "Does not fit any of the other concepts.";
/// A `choice` question allows at most 255 options, one of which is `other`.
pub const MAX_CONCEPTS: usize = 254;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Concept {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Concepts {
    #[serde(default, rename = "concept")]
    pub concepts: Vec<Concept>,
}

impl Concepts {
    /// Load and validate. A missing file is `Ok(None)`: the concept tasks
    /// then plan nothing.
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(Error::Io {
                    path: path.to_path_buf(),
                    source,
                })
            }
        };
        let parsed: Self = toml::from_str(&raw).map_err(|source| Error::ConfigParse {
            path: path.to_path_buf(),
            source,
        })?;
        parsed.validate().map_err(|message| Error::ConfigInvalid {
            path: path.to_path_buf(),
            message,
        })?;
        Ok(Some(parsed))
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.concepts.is_empty() {
            return Err("concepts.toml defines no [[concept]] entries".to_owned());
        }
        if self.concepts.len() > MAX_CONCEPTS {
            return Err(format!(
                "concepts.toml defines {} concepts; at most {MAX_CONCEPTS} fit one question",
                self.concepts.len()
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for c in &self.concepts {
            let valid_id = !c.id.is_empty()
                && c.id.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'
                });
            if !valid_id {
                return Err(format!(
                    "concept id `{}` must be lowercase ASCII letters, digits, `_` or `-`",
                    c.id
                ));
            }
            if c.description.trim().is_empty() {
                return Err(format!("concept `{}` needs a description", c.id));
            }
            if !seen.insert(c.id.as_str()) {
                return Err(format!("concept id `{}` is defined twice", c.id));
            }
        }
        Ok(())
    }

    /// `(id, description)` for every concept, then `other` unless the
    /// vocabulary already defines it. The order is the file's order.
    #[must_use]
    pub fn labels(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .concepts
            .iter()
            .map(|c| (c.id.clone(), c.description.trim().to_owned()))
            .collect();
        if !out.iter().any(|(id, _)| id == OTHER) {
            out.push((OTHER.to_owned(), OTHER_DESCRIPTION.to_owned()));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Concepts {
        toml::from_str(s).unwrap()
    }

    #[test]
    fn labels_append_other_once() {
        let c = parse("[[concept]]\nid = \"a\"\ndescription = \"A.\"\n");
        c.validate().unwrap();
        let labels = c.labels();
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[1].0, OTHER);
        let c = parse("[[concept]]\nid = \"other\"\ndescription = \"Misc.\"\n");
        assert_eq!(c.labels().len(), 1);
    }

    #[test]
    fn rejects_bad_ids_duplicates_and_empty() {
        assert!(parse("").validate().is_err());
        assert!(parse("[[concept]]\nid = \"Bad Id\"\ndescription = \"x\"\n")
            .validate()
            .is_err());
        assert!(parse("[[concept]]\nid = \"a\"\ndescription = \" \"\n")
            .validate()
            .is_err());
        assert!(parse("[[concept]]\nid = \"a\"\ndescription = \"x\"\n[[concept]]\nid = \"a\"\ndescription = \"y\"\n")
            .validate()
            .is_err());
        assert!(toml::from_str::<Concepts>(
            "[[concept]]\nid = \"a\"\ndescription = \"x\"\nextra = 1\n"
        )
        .is_err());
    }

    #[test]
    fn missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Concepts::load(&dir.path().join("concepts.toml"))
            .unwrap()
            .is_none());
    }
}
