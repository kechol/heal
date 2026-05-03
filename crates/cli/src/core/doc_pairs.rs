//! `.heal/doc_pairs.json` — single source of truth for Layer A doc ⇔
//! src pair mappings used by the `[features.docs]` observer family.
//!
//! The HEAL binary is a **read-only consumer** of this file. Generation
//! is the `/heal-doc-pair-setup` skill's responsibility (mention-based
//! regex, directory mirror heuristics, optional LLM inference). That
//! split keeps the binary deterministic — no heuristics, no model
//! calls, just JSON in / `Vec<DocPair>` out — and matches the
//! config-as-output pattern of `.heal/calibration.toml` (see
//! `scope.md` R3 / R6).
//!
//! ## File layout
//!
//! ```json
//! {
//!   "version": 1,
//!   "pairs": [
//!     { "doc": "docs/cli.md",
//!       "srcs": ["crates/cli/src/cli.rs"],
//!       "confidence": 0.9,
//!       "source": "mention" }
//!   ]
//! }
//! ```
//!
//! `version` bumps follow the same rules as `FINDINGS_RECORD_VERSION`:
//! a field rename or semantic change requires a bump and a CHANGELOG
//! migration note. Old files invalidate silently — the docs feature
//! prints a warning and continues with empty findings.
//!
//! ## Integrity warnings
//!
//! [`DocPairsFile::integrity_check`] returns one warning per referenced
//! path that does not exist on disk. The check is non-fatal: an entry
//! whose `doc` was renamed surfaces as a `doc_coverage` finding via
//! the dedicated observer; an entry whose `src` was deleted likewise
//! surfaces as drift. Hard-failing here would make every rename break
//! the build for one commit.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::error::{Error, Result};

/// Schema version for [`DocPairsFile`]. Bump on any breaking change to
/// the JSON shape — readers silently treat older versions as absent so
/// the user re-runs `/heal-doc-pair-setup` rather than seeing
/// half-decoded entries.
pub const DOC_PAIRS_VERSION: u32 = 1;

/// How a pair was discovered. The `manual` variant is load-bearing:
/// `/heal-doc-pair-setup` must preserve hand-authored entries on
/// regeneration so users can correct heuristics without losing the
/// fix on the next sweep.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PairSource {
    /// Doc literally references the src path or a symbol defined in it.
    Mention,
    /// Directory layout mirrors src to doc (e.g. `src/foo.rs` ↔
    /// `docs/foo.md`).
    Mirror,
    /// LLM inference filled the gap when no syntactic signal sufficed.
    Llm,
    /// User-authored. Preserved across regeneration.
    Manual,
}

/// One doc ⇔ src(s) entry. `srcs` is plural because a single doc page
/// often documents a small surface across several files (one CLI page
/// covering a `cli.rs` plus a `commands/` directory, say).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DocPair {
    /// Project-relative path to the doc file (forward-slash form).
    pub doc: String,
    /// One or more project-relative source files this doc describes.
    pub srcs: Vec<String>,
    /// Detection confidence in `[0.0, 1.0]`. Optional so manual
    /// entries can omit it without serializing a meaningless `1.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    /// How the entry was discovered. Optional for the same reason —
    /// older skill versions wrote no provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PairSource>,
}

/// Root of `.heal/doc_pairs.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DocPairsFile {
    pub version: u32,
    #[serde(default)]
    pub pairs: Vec<DocPair>,
}

/// One referenced path that doesn't exist on disk. Surfaces as a
/// stderr warning at scan time; the docs observers themselves keep
/// running with the offending entry skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocPairsWarning {
    /// Index of the offending entry within `pairs[]`. Stable for one
    /// run; not part of any persisted output.
    pub pair_index: usize,
    pub missing_path: PathBuf,
}

impl DocPairsFile {
    /// Read and deserialize `.heal/doc_pairs.json` (or whatever the
    /// `pairs_path` config resolves to).
    ///
    /// - `Ok(None)` — the file is absent. The caller is expected to
    ///   surface a hint pointing the user at `/heal-doc-pair-setup`.
    /// - `Ok(Some(_))` — file present and parsed.
    /// - `Err(_)` — file present but malformed. Returned as
    ///   `core::Error::CacheParse` (same family as `findings_cache`)
    ///   so callers don't have to reach into a separate error type.
    ///
    /// Schema-version mismatches return `Ok(None)` rather than an
    /// error: the contract is "stale shape ⇒ rerun the generator",
    /// matching `findings_cache::read_latest`.
    pub fn read(project: &Path, pairs_path: &str) -> Result<Option<Self>> {
        let abs = project.join(pairs_path);
        let raw = match std::fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(Error::Io { path: abs, source }),
        };
        let parsed: Self = serde_json::from_str(&raw).map_err(|source| Error::CacheParse {
            path: abs.clone(),
            source,
        })?;
        if parsed.version != DOC_PAIRS_VERSION {
            return Ok(None);
        }
        Ok(Some(parsed))
    }

    /// Verify every `doc` and every entry in `srcs` exists under
    /// `project`. Empty result = healthy `SSoT`.
    #[must_use]
    pub fn integrity_check(&self, project: &Path) -> Vec<DocPairsWarning> {
        let mut warnings = Vec::new();
        for (idx, pair) in self.pairs.iter().enumerate() {
            let doc_path = project.join(&pair.doc);
            if !doc_path.exists() {
                warnings.push(DocPairsWarning {
                    pair_index: idx,
                    missing_path: PathBuf::from(&pair.doc),
                });
            }
            for src in &pair.srcs {
                let src_path = project.join(src);
                if !src_path.exists() {
                    warnings.push(DocPairsWarning {
                        pair_index: idx,
                        missing_path: PathBuf::from(src),
                    });
                }
            }
        }
        warnings
    }

    /// Convenience for tests / the docs observers: pairs whose `doc`
    /// **and** every `srcs[]` entry exists. Drops malformed entries
    /// silently; pair the call with [`integrity_check`] when the
    /// caller wants user-visible warnings.
    ///
    /// [`integrity_check`]: Self::integrity_check
    #[must_use]
    pub fn live_pairs(&self, project: &Path) -> Vec<&DocPair> {
        self.pairs
            .iter()
            .filter(|p| {
                project.join(&p.doc).exists() && p.srcs.iter().all(|s| project.join(s).exists())
            })
            .collect()
    }
}
