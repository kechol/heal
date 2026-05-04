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
use crate::observer::code::hotspot::HotspotReport;
use crate::observer::docs::hotspot::DocHotspotReport;
use crate::observer::test::hotspot::TestHotspotReport;

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

/// Feature family. Drives per-family `HotspotIndex` dispatch in
/// [`FeatureRegistry::lower_all`] so a `coverage_pct` Finding picks
/// up `hotspot=true` from the [`Family::Test`] index, a `doc_drift`
/// Finding from the [`Family::Docs`] index, and a `ccn` Finding from
/// the [`Family::Code`] index. Also surfaced to user-facing
/// `--feature` filters in the v0.4 status / metrics flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Code,
    Test,
    Docs,
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
    /// Code-family Hotspot index (`commits × CCN_sum` per src file),
    /// calibrated against `cal.calibration.hotspot`.
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

    /// Test-family Hotspot index (`commits × uncov_pct` per src file),
    /// calibrated against `cal.calibration.test_hotspot`.
    #[must_use]
    pub fn for_test(report: Option<&TestHotspotReport>, cal: &Calibration) -> Self {
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
            calibration: cal.calibration.test_hotspot.clone(),
        }
    }

    /// Docs-family Hotspot index (`paired_src_churn × debt` per pair).
    /// The same score is registered under both the doc path and every
    /// paired src path so a `doc_freshness` Finding (primary = doc)
    /// and a `doc_drift` Finding whose `locations[]` touches a paired
    /// src both pick up the decoration. Calibrated against
    /// `cal.calibration.doc_hotspot`.
    #[must_use]
    pub fn for_doc(report: Option<&DocHotspotReport>, cal: &Calibration) -> Self {
        let mut by_path: std::collections::HashMap<PathBuf, f64> = std::collections::HashMap::new();
        if let Some(h) = report {
            for entry in &h.entries {
                let _ = by_path
                    .entry(entry.doc_path.clone())
                    .and_modify(|v| {
                        if entry.score > *v {
                            *v = entry.score;
                        }
                    })
                    .or_insert(entry.score);
                for src in &entry.src_paths {
                    let _ = by_path
                        .entry(src.clone())
                        .and_modify(|v| {
                            if entry.score > *v {
                                *v = entry.score;
                            }
                        })
                        .or_insert(entry.score);
                }
            }
        }
        Self {
            by_path,
            calibration: cal.calibration.doc_hotspot.clone(),
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
/// every Feature's lowering path; centralized here so the rule "hotspot
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
    /// Family this Feature belongs to. Drives per-family
    /// `HotspotIndex` dispatch in [`FeatureRegistry::lower_all`].
    /// Defaults to [`Family::Code`] so existing code-side Features
    /// keep working without a per-impl override; Test- and
    /// Docs-family Features override this method.
    fn family(&self) -> Family {
        Family::Code
    }
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
    /// All builtin Features. Order matters — same-Severity ties in the
    /// renderer fall back to it for stable output. Append new Features
    /// at the end to keep that contract.
    #[must_use]
    pub fn builtin() -> Self {
        use crate::observer::code::change_coupling::ChangeCouplingFeature;
        use crate::observer::code::complexity::ComplexityFeature;
        use crate::observer::code::duplication::DuplicationFeature;
        use crate::observer::code::hotspot::HotspotFeature;
        use crate::observer::code::lcom::LcomFeature;
        use crate::observer::docs::coverage::DocCoverageFeature;
        use crate::observer::docs::drift::DocDriftFeature;
        use crate::observer::docs::freshness::DocFreshnessFeature;
        use crate::observer::docs::hotspot::DocHotspotFeature;
        use crate::observer::docs::link_health::DocLinkHealthFeature;
        use crate::observer::docs::orphan_pages::OrphanPagesFeature;
        use crate::observer::docs::todo_density::TodoDensityFeature;
        use crate::observer::test::coverage::CoverageFeature;
        use crate::observer::test::hotspot::TestHotspotFeature;
        use crate::observer::test::skip_ratio::SkipRatioFeature;

        Self {
            features: vec![
                Box::new(ComplexityFeature),
                Box::new(DuplicationFeature),
                Box::new(ChangeCouplingFeature),
                Box::new(HotspotFeature),
                Box::new(LcomFeature),
                Box::new(DocFreshnessFeature),
                Box::new(DocDriftFeature),
                Box::new(DocCoverageFeature),
                Box::new(DocLinkHealthFeature),
                Box::new(OrphanPagesFeature),
                Box::new(TodoDensityFeature),
                Box::new(DocHotspotFeature),
                Box::new(CoverageFeature),
                Box::new(SkipRatioFeature),
                Box::new(TestHotspotFeature),
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
        let code_hotspot = HotspotIndex::new(reports.hotspot.as_ref(), cal);
        let test_hotspot = HotspotIndex::for_test(reports.test_hotspot.as_ref(), cal);
        let doc_hotspot = HotspotIndex::for_doc(reports.doc_hotspot.as_ref(), cal);
        let mut findings = Vec::new();
        for feature in self.enabled(cfg) {
            let idx = match feature.family() {
                Family::Code => &code_hotspot,
                Family::Test => &test_hotspot,
                Family::Docs => &doc_hotspot,
            };
            findings.extend(feature.lower(reports, cfg, cal, idx));
        }
        // When `[features.test]` is disabled, every finding keeps the
        // default `is_test_file = false` and the post-pass is skipped.
        if cfg.features.test.enabled {
            tag_test_findings(&mut findings, cfg);
        }
        findings
    }
}

/// Set [`Finding::is_test_file`] on every finding whose primary
/// `location.file` matches `cfg.features.test.test_paths` (gitignore
/// DSL). Glob compile errors fall back to the convention-based
/// [`crate::observer::shared::file_role::is_test_path`] heuristic so
/// a malformed user pattern doesn't suppress the flag entirely.
fn tag_test_findings(findings: &mut [Finding], cfg: &Config) {
    use crate::observer::shared::file_role::is_test_path;
    use crate::observer::shared::walk::ExcludeMatcher;

    let glob = if cfg.features.test.test_paths.is_empty() {
        None
    } else {
        ExcludeMatcher::compile(Path::new(""), &cfg.features.test.test_paths).ok()
    };
    for f in findings.iter_mut() {
        let path = &f.location.file;
        let hit = match glob.as_ref() {
            Some(m) => m.is_excluded(path, false),
            None => is_test_path(path),
        };
        if hit {
            f.is_test_file = true;
        }
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
        // on it for stable Finding ordering. Docs Features sit between
        // code and test so the v0.2 emission order for code metrics is
        // preserved; per-family hotspots (`doc_hotspot`, `test_hotspot`)
        // sit at the end of their own family blocks.
        assert_eq!(
            names,
            vec![
                "complexity",
                "duplication",
                "change_coupling",
                "hotspot",
                "lcom",
                "doc_freshness",
                "doc_drift",
                "doc_coverage",
                "doc_link_health",
                "orphan_pages",
                "todo_density",
                "doc_hotspot",
                "coverage_pct",
                "skip_ratio",
                "test_hotspot",
            ],
        );
    }

    #[test]
    fn every_code_feature_is_observer_kind() {
        // Code Features (the v0.2 set) are FeatureKind::Observer; docs
        // Features carry FeatureKind::DocsScanner; coverage_pct carries
        // FeatureKind::CoverageReader. The check is per-family rather
        // than per-Feature so adding a new code observer continues to
        // require Observer kind.
        for f in FeatureRegistry::builtin().iter() {
            let want_kind = match f.meta().name {
                "doc_freshness" | "doc_drift" | "doc_coverage" | "doc_link_health"
                | "orphan_pages" | "todo_density" | "doc_hotspot" => FeatureKind::DocsScanner,
                "coverage_pct" => FeatureKind::CoverageReader,
                _ => FeatureKind::Observer,
            };
            assert_eq!(
                f.meta().kind,
                want_kind,
                "unexpected kind for {}",
                f.meta().name,
            );
        }
    }

    #[test]
    fn family_assignment_per_feature_name() {
        for f in FeatureRegistry::builtin().iter() {
            let want_family = match f.meta().name {
                "doc_freshness" | "doc_drift" | "doc_coverage" | "doc_link_health"
                | "orphan_pages" | "todo_density" | "doc_hotspot" => Family::Docs,
                "coverage_pct" | "skip_ratio" | "test_hotspot" => Family::Test,
                _ => Family::Code,
            };
            assert_eq!(
                f.family(),
                want_family,
                "unexpected family for {}",
                f.meta().name,
            );
        }
    }
}
