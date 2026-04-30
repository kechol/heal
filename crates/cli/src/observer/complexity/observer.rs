//! Project-wide complexity scan: walks the source tree, parses every
//! supported file, and aggregates per-function CCN + Cognitive metrics.

use std::cmp::Reverse;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::config::Config;
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::core::severity::Severity;
use crate::feature::{decorate, Feature, FeatureKind, FeatureMeta, HotspotIndex};

use crate::observer::complexity::{analyze, parse, FunctionMetric};
use crate::observer::lang::Language;
use crate::observer::walk::walk_supported_files;
use crate::observer::{ObservationMeta, Observer};
use crate::observers::ObserverReports;

#[derive(Debug, Clone, Default)]
pub struct ComplexityObserver {
    /// Substrings checked against every visited path; matches are skipped.
    /// Mirrors `LocObserver::excluded` so excludes behave consistently.
    pub excluded: Vec<String>,
    pub ccn_enabled: bool,
    pub cognitive_enabled: bool,
}

impl ComplexityObserver {
    /// Inherit excludes from `git.exclude_paths` + `metrics.loc.exclude_paths`
    /// (matching `LocObserver::from_config`'s contract) and read the per-metric
    /// toggles from `metrics.ccn` / `metrics.cognitive`.
    #[must_use]
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            excluded: cfg.observer_excluded_paths(),
            ccn_enabled: cfg.metrics.ccn.enabled,
            cognitive_enabled: cfg.metrics.cognitive.enabled,
        }
    }

    /// Walk `root`, parse every supported file, and return per-file metrics.
    /// Returns `ComplexityReport::default()` if both toggles are disabled.
    #[must_use]
    pub fn scan(&self, root: &Path) -> ComplexityReport {
        if !self.ccn_enabled && !self.cognitive_enabled {
            return ComplexityReport::default();
        }

        let mut files: Vec<FileComplexity> = Vec::new();
        let mut totals = ComplexityTotals::default();
        for path in walk_supported_files(root, &self.excluded) {
            let lang = Language::from_path(&path).expect("walker filters by Language::from_path");
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(parsed) = parse(source, lang) else {
                continue;
            };
            let metrics = analyze(&parsed);
            if metrics.is_empty() {
                continue;
            }
            for fun in &metrics {
                totals.functions += 1;
                totals.max_ccn = totals.max_ccn.max(fun.ccn);
                totals.max_cognitive = totals.max_cognitive.max(fun.cognitive);
            }
            let rel = path
                .strip_prefix(root)
                .map(Path::to_path_buf)
                .unwrap_or(path);
            files.push(FileComplexity {
                path: rel,
                language: lang.name().to_string(),
                functions: metrics,
            });
        }
        totals.files = files.len();

        files.sort_by(|a, b| a.path.cmp(&b.path));
        ComplexityReport { files, totals }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComplexityReport {
    pub files: Vec<FileComplexity>,
    pub totals: ComplexityTotals,
}

impl ComplexityReport {
    /// Flat top-N view sorted by `metric` descending. Ties broken by file path
    /// then function name for determinism.
    #[must_use]
    pub fn worst_n(&self, n: usize, metric: ComplexityMetric) -> Vec<FunctionFinding> {
        let mut all: Vec<FunctionFinding> = self
            .files
            .iter()
            .flat_map(|file| {
                file.functions.iter().map(|fun| FunctionFinding {
                    file: file.path.clone(),
                    language: file.language.clone(),
                    name: fun.name.clone(),
                    line: fun.start_line,
                    ccn: fun.ccn,
                    cognitive: fun.cognitive,
                })
            })
            .collect();

        let key = |f: &FunctionFinding| match metric {
            ComplexityMetric::Ccn => f.ccn,
            ComplexityMetric::Cognitive => f.cognitive,
        };
        all.sort_by(|a, b| {
            Reverse(key(a))
                .cmp(&Reverse(key(b)))
                .then_with(|| a.file.cmp(&b.file))
                .then_with(|| a.name.cmp(&b.name))
        });
        all.truncate(n);
        all
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileComplexity {
    pub path: PathBuf,
    pub language: String,
    pub functions: Vec<FunctionMetric>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComplexityTotals {
    pub files: usize,
    pub functions: usize,
    pub max_ccn: u32,
    pub max_cognitive: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionFinding {
    pub file: PathBuf,
    pub language: String,
    pub name: String,
    pub line: u32,
    pub ccn: u32,
    pub cognitive: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplexityMetric {
    Ccn,
    Cognitive,
}

impl Observer for ComplexityObserver {
    type Output = ComplexityReport;

    fn meta(&self) -> ObservationMeta {
        ObservationMeta {
            name: "complexity",
            version: 1,
        }
    }

    fn observe(&self, project_root: &Path) -> anyhow::Result<Self::Output> {
        Ok(self.scan(project_root))
    }
}

impl IntoFindings for ComplexityReport {
    /// CCN and Cognitive are calibrated as separate metrics (TODO
    /// §Severity), so each function above zero on either axis becomes
    /// its own finding. The content seed is the function span size, so
    /// a function moved to a different line still hashes the same as
    /// long as its size is unchanged.
    fn into_findings(&self) -> Vec<Finding> {
        let mut out = Vec::with_capacity(self.totals.functions);
        for file in &self.files {
            for fun in &file.functions {
                let span = fun.end_line.saturating_sub(fun.start_line);
                let location = Location {
                    file: file.path.clone(),
                    line: Some(fun.start_line),
                    symbol: Some(fun.name.clone()),
                };
                if fun.ccn > 0 {
                    out.push(Finding::new(
                        "ccn",
                        location.clone(),
                        format!("CCN={} {} ({})", fun.ccn, fun.name, file.language),
                        &format!("ccn:{span}"),
                    ));
                }
                if fun.cognitive > 0 {
                    out.push(Finding::new(
                        "cognitive",
                        location,
                        format!(
                            "Cognitive={} {} ({})",
                            fun.cognitive, fun.name, file.language
                        ),
                        &format!("cognitive:{span}"),
                    ));
                }
            }
        }
        out
    }
}

/// Feature wrapper covering both CCN and Cognitive dimensions of
/// [`ComplexityReport`]. The underlying scan runs once per project
/// (it parses every supported file with tree-sitter); per-metric
/// toggles `cfg.metrics.ccn.enabled` / `cfg.metrics.cognitive.enabled`
/// gate the respective Findings inside `lower`.
pub struct ComplexityFeature;

impl Feature for ComplexityFeature {
    fn meta(&self) -> FeatureMeta {
        FeatureMeta {
            name: "complexity",
            version: 1,
            kind: FeatureKind::Observer,
        }
    }
    fn enabled(&self, cfg: &Config) -> bool {
        cfg.metrics.ccn.enabled || cfg.metrics.cognitive.enabled
    }
    fn lower(
        &self,
        reports: &ObserverReports,
        cfg: &Config,
        cal: &crate::core::calibration::Calibration,
        hotspot: &HotspotIndex,
    ) -> Vec<Finding> {
        let cal_ccn = cal.calibration.ccn.as_ref();
        let cal_cog = cal.calibration.cognitive.as_ref();
        let mut out = Vec::new();
        for file in &reports.complexity.files {
            for fun in &file.functions {
                let span = fun.end_line.saturating_sub(fun.start_line);
                let location = Location {
                    file: file.path.clone(),
                    line: Some(fun.start_line),
                    symbol: Some(fun.name.clone()),
                };
                if cfg.metrics.ccn.enabled && fun.ccn > 0 {
                    let f = Finding::new(
                        "ccn",
                        location.clone(),
                        format!("CCN={} {} ({})", fun.ccn, fun.name, file.language),
                        &format!("ccn:{span}"),
                    );
                    let sev = cal_ccn.map_or(Severity::Ok, |c| c.classify(f64::from(fun.ccn)));
                    out.push(decorate(f, sev, hotspot));
                }
                if cfg.metrics.cognitive.enabled && fun.cognitive > 0 {
                    let f = Finding::new(
                        "cognitive",
                        location,
                        format!(
                            "Cognitive={} {} ({})",
                            fun.cognitive, fun.name, file.language
                        ),
                        &format!("cognitive:{span}"),
                    );
                    let sev =
                        cal_cog.map_or(Severity::Ok, |c| c.classify(f64::from(fun.cognitive)));
                    out.push(decorate(f, sev, hotspot));
                }
            }
        }
        out
    }
}
