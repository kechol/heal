//! Cross-observer `Finding` abstraction — promoted to `core` as the
//! prerequisite for v0.2 (Calibration, `heal status` cache,
//! `/heal-code-patch`).
//!
//! Every observer's report can be lowered into `Vec<Finding>` via the
//! [`IntoFindings`] trait. The lowering is deterministic and pure: it
//! does not consult Calibration, it does not classify severity, and it
//! does not flag hotspots. Those layers attach **on top** of a Finding
//! list — see TODO §Severity と Calibration. Until they land, every
//! emitted finding carries `severity = Severity::Ok` and `hotspot =
//! false`.
//!
//! `Finding::id` is **decision-stable**: identical input (metric +
//! canonical location + an observer-supplied content seed) hashes to
//! the same string across processes, toolchains, and commits. The
//! cache layer relies on this so a re-detected finding ties back to
//! its prior occurrence — see TODO §Result cache.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::core::hash::{fnv1a_64_chunked, fnv1a_hex};
pub use crate::core::severity::Severity;

/// A single point in the codebase a finding refers to. `line` and
/// `symbol` are optional because not every metric has them — hotspot
/// is file-level, duplication knows ranges but no symbol, complexity
/// has both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub file: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

impl Location {
    #[must_use]
    pub fn file(file: PathBuf) -> Self {
        Self {
            file,
            line: None,
            symbol: None,
        }
    }
}

/// One actionable signal produced by an observer. Multi-site findings
/// (duplication blocks, coupling pairs) carry the canonical
/// representative in `location` and the rest in `locations`; the id
/// is derived from `location` + a metric-specific content seed so
/// alternative orderings of the same set hash identically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub metric: String,
    #[serde(default)]
    pub severity: Severity,
    #[serde(default)]
    pub hotspot: bool,
    /// Workspace path (project-root relative) when the finding's
    /// `location.file` lives under a declared `[[project.workspaces]]`
    /// entry. `None` for files outside every declared workspace, or
    /// when no workspaces are declared. Tagged post-classify by
    /// [`crate::core::config::assign_workspace`]; not part of the id
    /// hash, so reclassifying a workspace boundary doesn't churn the
    /// fix-tracking history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub location: Location,
    /// Sites beyond the canonical `location`. Populated for duplication
    /// blocks (other duplicates) and coupling pairs (the partner file).
    /// Skipped from JSON when empty so single-site findings stay terse.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<Location>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_hint: Option<String>,
}

impl Finding {
    /// Build a finding with the v0.2-prerequisite defaults: `Severity::Ok`
    /// (Calibration assigns the real severity later), `hotspot = false`,
    /// no extra locations, and no fix hint. The id is derived eagerly
    /// from `(metric, location, content_seed)` via [`Self::make_id`].
    /// Multi-site findings layer extras on with [`Self::with_locations`].
    #[must_use]
    pub fn new(metric: &str, location: Location, summary: String, content_seed: &str) -> Self {
        let id = Self::make_id(metric, &location, content_seed);
        Self {
            id,
            metric: metric.to_owned(),
            severity: Severity::Ok,
            hotspot: false,
            workspace: None,
            location,
            locations: Vec::new(),
            summary,
            fix_hint: None,
        }
    }

    #[must_use]
    pub fn with_locations(mut self, extras: Vec<Location>) -> Self {
        self.locations = extras;
        self
    }

    /// Compact "metric=N" tag used by `heal status` rows and the
    /// post-commit nudge. The numeric tail is recovered from
    /// `summary` so observers don't have to expose a second value
    /// channel; metrics whose summary doesn't carry a leading number
    /// (`duplication`, `change_coupling`, `hotspot`) fall back to a
    /// short label.
    #[must_use]
    pub fn short_label(&self) -> String {
        match self.metric.as_str() {
            "ccn" => extract_leading_number(&self.summary, "CCN=")
                .map_or_else(|| "CCN".to_owned(), |v| format!("CCN={v}")),
            "cognitive" => extract_leading_number(&self.summary, "Cognitive=")
                .map_or_else(|| "Cognitive".to_owned(), |v| format!("Cognitive={v}")),
            "duplication" => "duplication".to_owned(),
            "change_coupling" => "coupled".to_owned(),
            "change_coupling.symmetric" => "coupled (sym)".to_owned(),
            "hotspot" => "hotspot".to_owned(),
            "lcom" => extract_leading_number(&self.summary, "LCOM=")
                .map_or_else(|| "LCOM".to_owned(), |v| format!("LCOM={v}")),
            other => other.to_owned(),
        }
    }

    /// Compose the stable id for a finding.
    ///
    /// Format: `<metric>:<file>:<symbol-or-*>:<16-hex-fnv1a>`. The hex
    /// digest covers `metric || file || symbol || content_seed`, so
    /// even when two findings share a (metric, file, symbol) triple
    /// the seed differentiates them. Conversely, an unchanged finding
    /// across commits hashes identically because the inputs are
    /// observer-derived strings, not line numbers or scores.
    #[must_use]
    pub fn make_id(metric: &str, location: &Location, content_seed: &str) -> String {
        let path = location.file.to_string_lossy();
        let symbol = location.symbol.as_deref().unwrap_or("*");
        let h = fnv1a_64_chunked(&[
            metric.as_bytes(),
            path.as_bytes(),
            symbol.as_bytes(),
            content_seed.as_bytes(),
        ]);
        format!("{metric}:{path}:{symbol}:{}", fnv1a_hex(h))
    }
}

/// Pluck the `<digits>` immediately after `prefix` from `summary`,
/// returning `None` when no digit follows. Used by [`Finding::short_label`]
/// to recover CCN/Cognitive numbers without round-tripping observer state.
fn extract_leading_number(summary: &str, prefix: &str) -> Option<String> {
    let after = summary.strip_prefix(prefix)?;
    let value: String = after.chars().take_while(char::is_ascii_digit).collect();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Lower an observer report into a list of findings.
///
/// Implementations live next to each observer report (`observer::*`).
/// The trait is sealed by convention — only HEAL's own observers are
/// expected to implement it.
///
/// The method takes `&self` (not `self`) because callers usually keep
/// the report around for `heal metrics` rendering after extracting
/// findings.
pub trait IntoFindings {
    #[allow(clippy::wrong_self_convention)]
    fn into_findings(&self) -> Vec<Finding>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(file: &str, symbol: Option<&str>, line: Option<u32>) -> Location {
        Location {
            file: PathBuf::from(file),
            line,
            symbol: symbol.map(str::to_owned),
        }
    }

    #[test]
    fn make_id_is_stable_for_identical_input() {
        let l = loc("src/foo.rs", Some("bar"), Some(10));
        let a = Finding::make_id("ccn", &l, "seed-1");
        let b = Finding::make_id("ccn", &l, "seed-1");
        assert_eq!(a, b);
        assert!(a.starts_with("ccn:src/foo.rs:bar:"));
    }

    #[test]
    fn make_id_differs_when_any_component_differs() {
        let l = loc("src/foo.rs", Some("bar"), None);
        let base = Finding::make_id("ccn", &l, "");
        assert_ne!(base, Finding::make_id("cognitive", &l, ""));
        assert_ne!(
            base,
            Finding::make_id("ccn", &loc("src/baz.rs", Some("bar"), None), "")
        );
        assert_ne!(
            base,
            Finding::make_id("ccn", &loc("src/foo.rs", Some("baz"), None), "")
        );
        assert_ne!(base, Finding::make_id("ccn", &l, "extra"));
    }

    #[test]
    fn make_id_avoids_concatenation_collisions() {
        // Without separators, ("ab","c") and ("a","bc") would collide.
        let a = Finding::make_id("ab", &loc("c", None, None), "");
        let b = Finding::make_id("a", &loc("bc", None, None), "");
        assert_ne!(a, b);
    }

    #[test]
    fn make_id_uses_star_when_symbol_missing() {
        let l = loc("src/foo.rs", None, None);
        let id = Finding::make_id("hotspot", &l, "");
        assert!(id.starts_with("hotspot:src/foo.rs:*:"));
    }

    #[test]
    fn short_label_extracts_metric_number_or_falls_back() {
        let mut ccn = Finding::new(
            "ccn",
            loc("src/foo.rs", Some("bar"), Some(10)),
            "CCN=28 bar (rust)".into(),
            "seed",
        );
        ccn.severity = Severity::Critical;
        assert_eq!(ccn.short_label(), "CCN=28");

        let cog = Finding::new(
            "cognitive",
            loc("src/foo.rs", Some("bar"), Some(10)),
            "Cognitive=42 bar (rust)".into(),
            "seed",
        );
        assert_eq!(cog.short_label(), "Cognitive=42");

        // Summary with no digit after `CCN=` falls back to the bare label.
        let bare = Finding::new(
            "ccn",
            loc("src/foo.rs", Some("bar"), Some(10)),
            "no number here".into(),
            "seed",
        );
        assert_eq!(bare.short_label(), "CCN");

        let dup = Finding::new(
            "duplication",
            loc("src/foo.rs", None, None),
            "anything".into(),
            "",
        );
        assert_eq!(dup.short_label(), "duplication");
    }

    #[test]
    fn finding_serialises_without_empty_locations_or_fix_hint() {
        let f = Finding {
            id: "x".into(),
            metric: "ccn".into(),
            severity: Severity::Ok,
            hotspot: false,
            workspace: None,
            location: loc("src/foo.rs", Some("bar"), Some(1)),
            locations: vec![],
            summary: "hi".into(),
            fix_hint: None,
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(!json.contains("locations"));
        assert!(!json.contains("fix_hint"));
        assert!(!json.contains("workspace"));
    }
}
