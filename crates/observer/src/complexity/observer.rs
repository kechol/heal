//! Project-wide complexity scan: walks the source tree, parses every
//! supported file, and aggregates per-function CCN + Cognitive metrics.

use std::cmp::Reverse;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use heal_core::config::Config;

use crate::complexity::{analyze, parse, FunctionMetric};
use crate::lang::Language;
use crate::walk::walk_supported_files;
use crate::{ObservationMeta, Observer};

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
