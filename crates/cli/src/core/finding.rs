//! Cross-observer `Finding` abstraction — promoted to `core` as the
//! prerequisite for v0.2 (Calibration, `heal check` cache, `/heal-fix`).
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

/// FNV-1a 64-bit constants — same prime/offset as
/// `observer::duplication`. We do not use `std::hash::DefaultHasher`
/// because it is explicitly unstable across Rust releases (see
/// `CLAUDE.md` §Hashing); a `rustc` upgrade would otherwise invalidate
/// every recorded id.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// Severity classes used by Calibration. Ordered `Ok < Medium < High <
/// Critical` so per-file aggregation can use `cmp::max`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default, Hash,
)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Ok,
    Medium,
    High,
    Critical,
}

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

        let mut h = FNV_OFFSET;
        for chunk in [
            metric.as_bytes(),
            path.as_bytes(),
            symbol.as_bytes(),
            content_seed.as_bytes(),
        ] {
            for b in chunk {
                h = (h ^ u64::from(*b)).wrapping_mul(FNV_PRIME);
            }
            // Field separator so `("ab", "c")` and `("a", "bc")` don't collide.
            h = (h ^ 0xff).wrapping_mul(FNV_PRIME);
        }
        format!("{metric}:{path}:{symbol}:{h:016x}")
    }
}

/// Lower an observer report into a list of findings.
///
/// Implementations live next to each observer report (`observer::*`).
/// The trait is sealed by convention — only HEAL's own observers are
/// expected to implement it.
///
/// The method takes `&self` (not `self`) because callers usually keep
/// the report around for `heal status` rendering after extracting
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
    fn severity_ord_runs_ok_to_critical() {
        assert!(Severity::Ok < Severity::Medium);
        assert!(Severity::Medium < Severity::High);
        assert!(Severity::High < Severity::Critical);
        assert_eq!(
            Severity::Critical,
            std::cmp::max(Severity::Ok, Severity::Critical)
        );
    }

    #[test]
    fn severity_default_is_ok() {
        assert_eq!(Severity::default(), Severity::Ok);
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
    fn finding_serialises_without_empty_locations_or_fix_hint() {
        let f = Finding {
            id: "x".into(),
            metric: "ccn".into(),
            severity: Severity::Ok,
            hotspot: false,
            location: loc("src/foo.rs", Some("bar"), Some(1)),
            locations: vec![],
            summary: "hi".into(),
            fix_hint: None,
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(!json.contains("locations"));
        assert!(!json.contains("fix_hint"));
    }
}
