//! Project-wide complexity scan: walks the source tree, parses every
//! supported file, and aggregates per-function CCN + Cognitive metrics.

use std::cmp::Reverse;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::config::Config;
use crate::core::finding::{Finding, IntoFindings, Location};
use crate::core::severity::Severity;
use crate::feature::{decorate, Feature, FeatureKind, FeatureMeta, HotspotIndex};

use crate::observer::code::complexity::{analyze, FunctionMetric, ParsedFile};
use crate::observer::shared::lang::Language;
use crate::observer::{impl_workspace_builder, ObservationMeta, Observer};
use crate::observers::ObserverReports;

impl_workspace_builder!(ComplexityObserver);

#[derive(Debug, Clone, Default)]
pub struct ComplexityObserver {
    /// Gitignore-style exclude lines applied to every visited path.
    /// Mirrors `LocObserver::excluded` so excludes behave consistently.
    pub excluded: Vec<String>,
    pub ccn_enabled: bool,
    pub cognitive_enabled: bool,
    /// Optional workspace sub-path. When set, the walk skips files
    /// outside this directory (segment-wise prefix). Used by
    /// `heal metrics --workspace <path>`.
    pub workspace: Option<PathBuf>,
}

impl ComplexityObserver {
    /// Inherit excludes from `git.exclude_paths` + `metrics.loc.exclude_paths`
    /// (matching `LocObserver::from_config`'s contract) and read the per-metric
    /// toggles from `metrics.ccn` / `metrics.cognitive`.
    #[must_use]
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            excluded: cfg.exclude_lines(),
            ccn_enabled: cfg.metrics.is_enabled("ccn"),
            cognitive_enabled: cfg.metrics.is_enabled("cognitive"),
            workspace: None,
        }
    }

    /// Walk `root`, parse every supported file, and return per-file metrics.
    /// Returns `ComplexityReport::default()` if both toggles are disabled.
    #[must_use]
    pub fn scan(&self, root: &Path) -> ComplexityReport {
        let Some(mut acc) = self.accumulator() else {
            return ComplexityReport::default();
        };
        crate::observer::code::scan_source_tree(
            root,
            &self.excluded,
            self.workspace.as_deref(),
            Some(&mut acc),
            None,
            None,
        );
        acc.finish()
    }

    /// Streaming half of [`Self::scan`]: `None` when both toggles are
    /// disabled, otherwise an accumulator the orchestrator feeds from
    /// the shared walk+parse pass (see
    /// [`crate::observer::code::scan_source_tree`]).
    pub(crate) fn accumulator(&self) -> Option<ComplexityAccumulator> {
        (self.ccn_enabled || self.cognitive_enabled).then(ComplexityAccumulator::default)
    }
}

/// Per-file accumulator behind [`ComplexityObserver::scan`]. Split out
/// so Complexity / Duplication / LCOM can share one walk+parse pass —
/// each observer used to re-read and re-parse every source file
/// independently, tripling the tree-sitter cost per run.
#[derive(Default)]
pub(crate) struct ComplexityAccumulator {
    files: Vec<FileComplexity>,
    totals: ComplexityTotals,
}

impl ComplexityAccumulator {
    pub(crate) fn add(&mut self, rel: &Path, lang: Language, parsed: &ParsedFile) {
        let metrics = analyze(parsed);
        if metrics.is_empty() {
            return;
        }
        for fun in &metrics {
            self.totals.functions += 1;
            self.totals.max_ccn = self.totals.max_ccn.max(fun.ccn);
            self.totals.max_cognitive = self.totals.max_cognitive.max(fun.cognitive);
        }
        self.files.push(FileComplexity {
            path: rel.to_path_buf(),
            language: lang.name().to_string(),
            functions: metrics,
        });
    }

    pub(crate) fn finish(mut self) -> ComplexityReport {
        self.totals.files = self.files.len();
        self.files.sort_by(|a, b| a.path.cmp(&b.path));
        ComplexityReport {
            files: self.files,
            totals: self.totals,
        }
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

/// Per-function content-seed suffixes for one file's complexity
/// findings. The base is the function's line span, so a function moved
/// to a different line keeps its id as long as its size is unchanged.
/// `Finding::make_id` doesn't hash `location.line`, so two functions
/// sharing name AND span in the same file (e.g. one-line `fn new` in
/// two different impl blocks) would otherwise collide to one id and
/// silently shadow each other in every id-keyed consumer (`fixed.json`
/// reconciliation, `heal diff` buckets). An occurrence ordinal is
/// appended from the second repeat on; first occurrences keep the
/// plain-span seed, so pre-existing non-colliding ids are unchanged.
fn function_seed_suffixes(functions: &[FunctionMetric]) -> Vec<String> {
    let mut seen: std::collections::HashMap<(&str, u32), u32> = std::collections::HashMap::new();
    functions
        .iter()
        .map(|fun| {
            let span = fun.end_line.saturating_sub(fun.start_line);
            let ordinal = seen
                .entry((fun.name.as_str(), span))
                .and_modify(|n| *n += 1)
                .or_insert(0);
            if *ordinal == 0 {
                format!("{span}")
            } else {
                format!("{span}:{ordinal}")
            }
        })
        .collect()
}

impl IntoFindings for ComplexityReport {
    /// CCN and Cognitive are calibrated as separate metrics (TODO
    /// §Severity), so each function above zero on either axis becomes
    /// its own finding. The content seed is the function span size
    /// (plus a collision ordinal — see [`function_seed_suffixes`]), so
    /// a function moved to a different line still hashes the same as
    /// long as its size is unchanged.
    fn into_findings(&self) -> Vec<Finding> {
        let mut out = Vec::with_capacity(self.totals.functions);
        for file in &self.files {
            let seeds = function_seed_suffixes(&file.functions);
            for (fun, seed) in file.functions.iter().zip(&seeds) {
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
                        &format!("ccn:{seed}"),
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
                        &format!("cognitive:{seed}"),
                    ));
                }
            }
        }
        out
    }
}

/// Feature wrapper covering both CCN and Cognitive dimensions of
/// [`ComplexityReport`]. The underlying scan runs once per project
/// (it parses every supported file with tree-sitter); the
/// `metrics.disabled` list (queried via
/// `cfg.metrics.is_enabled("ccn")` / `is_enabled("cognitive")`)
/// gates the respective Findings inside `lower`.
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
        cfg.metrics.is_enabled("ccn") || cfg.metrics.is_enabled("cognitive")
    }
    fn lower(
        &self,
        reports: &ObserverReports,
        cfg: &Config,
        cal: &crate::core::calibration::Calibration,
        hotspot: &HotspotIndex,
    ) -> Vec<Finding> {
        let workspaces = cfg.project.workspaces.as_slice();
        let mut out = Vec::new();
        for file in &reports.complexity.files {
            let metrics = cal.metrics_for_file(&file.path, workspaces);
            let cal_ccn = metrics.ccn.as_ref();
            let cal_cog = metrics.cognitive.as_ref();
            let seeds = function_seed_suffixes(&file.functions);
            for (fun, seed) in file.functions.iter().zip(&seeds) {
                let location = Location {
                    file: file.path.clone(),
                    line: Some(fun.start_line),
                    symbol: Some(fun.name.clone()),
                };
                if cfg.metrics.is_enabled("ccn") && fun.ccn > 0 {
                    let f = Finding::new(
                        "ccn",
                        location.clone(),
                        format!("CCN={} {} ({})", fun.ccn, fun.name, file.language),
                        &format!("ccn:{seed}"),
                    );
                    let sev = cal_ccn.map_or(Severity::Ok, |c| c.classify(f64::from(fun.ccn)));
                    out.push(decorate(f, sev, hotspot));
                }
                if cfg.metrics.is_enabled("cognitive") && fun.cognitive > 0 {
                    let f = Finding::new(
                        "cognitive",
                        location,
                        format!(
                            "Cognitive={} {} ({})",
                            fun.cognitive, fun.name, file.language
                        ),
                        &format!("cognitive:{seed}"),
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
