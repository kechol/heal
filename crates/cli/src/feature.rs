//! `Feature` — the unified interface that v0.3+ docs / coverage / lcov
//! readers will plug into and that v0.2's existing observers register
//! through.
//!
//! A Feature owns the lowering of one observer's typed report into
//! [`Vec<Finding>`] with severity + hotspot decoration applied. The
//! [`FeatureRegistry`] enumerates every builtin Feature and dispatches
//! the per-Feature `lower` step against a shared [`ObserverReports`]
//! plus [`Calibration`]. That replaces the inline per-metric branches
//! that used to live in `crate::observers::classify` — adding a new
//! metric (or, in v0.3, a new docs / coverage scanner) is now "implement
//! the trait + register".
//!
//! The runtime keeps `ObserverReports` as the inter-Feature glue
//! (Hotspot composition needs the typed Churn + Complexity reports, not
//! their Findings). User-facing output is always `Vec<Finding>`.

use std::path::{Path, PathBuf};

use crate::core::calibration::{Calibration, HotspotCalibration};
use crate::core::config::Config;
use crate::core::finding::{Finding, Location};
use crate::core::severity::Severity;
use crate::observer::hotspot::HotspotReport;

/// Cheap, copyable metadata. Identifies the Feature in the registry
/// listing and tags the records the runtime writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureMeta {
    pub name: &'static str,
    pub version: u32,
    pub kind: FeatureKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureKind {
    /// Reads project source / git history; this is the v0.2 default.
    Observer,
    /// Reads docs artifacts and source mtimes. Reserved for v0.3 — the
    /// storage and ingest path are TBD.
    DocsScanner,
    /// Reads lcov coverage files and emits Findings for low-coverage
    /// code. Reserved for v0.3.
    CoverageReader,
}

/// Per-file hotspot decoration index. Built once per `lower_all` pass
/// and threaded into each Feature so individual Findings can flip
/// `hotspot = true` without re-loading the hotspot report.
#[derive(Debug, Default)]
pub struct HotspotIndex {
    by_path: std::collections::HashMap<PathBuf, f64>,
    calibration: Option<HotspotCalibration>,
}

impl HotspotIndex {
    #[must_use]
    pub fn new(report: Option<&HotspotReport>, cal: &Calibration) -> Self {
        let by_path = report
            .map(|h| {
                h.entries
                    .iter()
                    .map(|e| (e.path.clone(), e.score))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            by_path,
            calibration: cal.calibration.hotspot.clone(),
        }
    }

    /// Whether `path`'s hotspot score crosses the calibration's `p90`.
    /// Returns `false` for files outside the index or when the project
    /// has no hotspot calibration yet.
    #[must_use]
    pub fn is_hot(&self, path: &Path) -> bool {
        match (&self.calibration, self.by_path.get(path)) {
            (Some(c), Some(score)) => c.flag(*score),
            _ => false,
        }
    }

    /// Convenience: a Finding's primary file or any of its
    /// `locations` is hot. `pub(crate)` because [`decorate`] is the
    /// only intended consumer; if v0.3 features need it, the contract
    /// can widen with intent.
    #[must_use]
    pub(crate) fn any_location_hot(&self, primary: &Location, locations: &[Location]) -> bool {
        self.is_hot(&primary.file) || locations.iter().any(|l| self.is_hot(&l.file))
    }
}

/// Apply Severity + hotspot decoration to a Finding in place. Used by
/// every Feature's lowering path; centralised here so the rule "hotspot
/// looks at primary file + every secondary location" only lives once.
/// `any_location_hot` short-circuits to `is_hot(primary)` when the
/// `locations` slice is empty, so single-site Findings flow through
/// the same path as multi-site ones without a special case.
#[must_use]
pub fn decorate(mut f: Finding, severity: Severity, hotspot: &HotspotIndex) -> Finding {
    f.severity = severity;
    f.hotspot = hotspot.any_location_hot(&f.location, &f.locations);
    f
}

/// The shared interface every metric (and v0.3 docs / coverage reader)
/// implements. The lifecycle the runtime drives is:
///
/// 1. `enabled(cfg)` — registry filter; disabled features never run.
/// 2. The runtime computes `ObserverReports` (cross-feature data —
///    Hotspot composition reads Churn + Complexity raw, etc.).
/// 3. `lower(reports, cfg, cal, hotspot)` — emit decorated Findings.
///
/// Two Features can share underlying observer state (CCN and
/// Cognitive both read `reports.complexity`), and that's intentional.
pub trait Feature: Send + Sync {
    fn meta(&self) -> FeatureMeta;
    fn enabled(&self, cfg: &Config) -> bool;
    /// Lower this Feature's slice of `reports` into Findings.
    /// Returns an empty Vec when the underlying observer didn't run
    /// (e.g. the Feature is enabled but its observer report is missing).
    fn lower(
        &self,
        reports: &crate::observers::ObserverReports,
        cfg: &Config,
        cal: &Calibration,
        hotspot: &HotspotIndex,
    ) -> Vec<Finding>;
}

/// Static registry of every builtin Feature. The order is the order
/// findings are emitted in `Vec<Finding>` — same-Severity tiebreakers
/// in the renderer fall back to it for determinism.
pub struct FeatureRegistry {
    features: Vec<Box<dyn Feature>>,
}

impl FeatureRegistry {
    /// All v0.2 builtin Features. Order matches the v0.1 `classify`
    /// emission order — same-Severity tiebreakers in the renderer
    /// rely on it.
    #[must_use]
    pub fn builtin() -> Self {
        use crate::observer::change_coupling::ChangeCouplingFeature;
        use crate::observer::complexity::ComplexityFeature;
        use crate::observer::duplication::DuplicationFeature;
        use crate::observer::hotspot::HotspotFeature;
        use crate::observer::lcom::LcomFeature;

        Self {
            features: vec![
                Box::new(ComplexityFeature),
                Box::new(DuplicationFeature),
                Box::new(ChangeCouplingFeature),
                Box::new(HotspotFeature),
                Box::new(LcomFeature),
            ],
        }
    }

    /// Iterator of every registered Feature regardless of enabled state.
    pub fn iter(&self) -> impl Iterator<Item = &dyn Feature> {
        self.features.iter().map(std::convert::AsRef::as_ref)
    }

    /// Iterator filtered to features the supplied config enables.
    pub fn enabled<'a>(&'a self, cfg: &'a Config) -> impl Iterator<Item = &'a dyn Feature> + 'a {
        self.iter().filter(move |f| f.enabled(cfg))
    }

    /// Drive every enabled Feature's `lower` against shared inputs and
    /// concatenate the Findings. The replacement entry point for the
    /// pre-v0.2 `crate::observers::classify`.
    pub fn lower_all(
        &self,
        reports: &crate::observers::ObserverReports,
        cfg: &Config,
        cal: &Calibration,
    ) -> Vec<Finding> {
        let hotspot = HotspotIndex::new(reports.hotspot.as_ref(), cal);
        let mut findings = Vec::new();
        for feature in self.enabled(cfg) {
            findings.extend(feature.lower(reports, cfg, cal, &hotspot));
        }
        findings
    }
}

impl Default for FeatureRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_emits_one_feature_per_metric() {
        let r = FeatureRegistry::builtin();
        let names: Vec<&str> = r.iter().map(|f| f.meta().name).collect();
        // Order is the public emission contract — tests / renderer rely
        // on it for stable Finding ordering.
        assert_eq!(
            names,
            vec![
                "complexity",
                "duplication",
                "change_coupling",
                "hotspot",
                "lcom",
            ],
        );
    }

    #[test]
    fn every_builtin_is_observer_kind() {
        for f in FeatureRegistry::builtin().iter() {
            assert_eq!(
                f.meta().kind,
                FeatureKind::Observer,
                "unexpected kind for {}",
                f.meta().name,
            );
        }
    }
}
