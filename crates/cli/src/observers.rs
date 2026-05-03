//! Single source of truth for "run every enabled observer". `heal
//! status`, `heal diff`, and the post-commit nudge all funnel through
//! [`run_all`] / [`build_record`] so a new observer or enable-flag
//! only needs editing in one place.

use std::collections::BTreeMap;
use std::path::Path;

use crate::core::calibration::{
    Calibration, CalibrationMeta, HotspotCalibration, MetricCalibration, MetricCalibrations,
    MetricFloors, FLOOR_CCN, FLOOR_COGNITIVE, FLOOR_DUPLICATION_PCT, FLOOR_OK_CCN,
    FLOOR_OK_COGNITIVE, STRATEGY_PERCENTILE,
};
use crate::core::config::Config;
use crate::core::finding::Finding;
use crate::observer::change_coupling::{ChangeCouplingObserver, ChangeCouplingReport};
use crate::observer::churn::{ChurnObserver, ChurnReport};
use crate::observer::complexity::{ComplexityObserver, ComplexityReport};
use crate::observer::duplication::{DuplicationObserver, DuplicationReport};
use crate::observer::hotspot::{compose as compose_hotspot, HotspotReport, HotspotWeights};
use crate::observer::lcom::{LcomObserver, LcomReport};
use crate::observer::loc::{LocObserver, LocReport};

use crate::cli::MetricKind;

pub struct ObserverReports {
    pub loc: LocReport,
    pub complexity: ComplexityReport,
    pub complexity_observer: ComplexityObserver,
    pub churn: Option<ChurnReport>,
    pub change_coupling: Option<ChangeCouplingReport>,
    pub duplication: Option<DuplicationReport>,
    pub hotspot: Option<HotspotReport>,
    pub lcom: Option<LcomReport>,
}

/// Run the observers needed for the requested metric. `only = None`
/// means "run everything" (the default, used by `heal status` and
/// the post-commit nudge). When `only` is set, observers irrelevant
/// to that metric are skipped —
/// churn and complexity still run for `Hotspot` because the composite
/// is built on top of them. The skipped observers' fields fall back to
/// `Default` (or `None` for the optional ones).
///
/// `workspace = Some(path)` (used by `heal metrics --workspace`)
/// scopes every observer's internal walk or git diff to files under
/// the sub-path, so totals, pair counts, and lift reflect only the
/// chosen workspace's universe. Pass `None` for whole-repo behavior.
pub(crate) fn run_all(
    project: &Path,
    cfg: &Config,
    only: Option<MetricKind>,
    workspace: Option<&Path>,
) -> ObserverReports {
    let want = |m: MetricKind| match only {
        None => true,
        Some(o) if o == m => true,
        Some(MetricKind::Hotspot) if matches!(m, MetricKind::Churn | MetricKind::Complexity) => {
            true
        }
        _ => false,
    };

    let ws_buf = workspace.map(Path::to_path_buf);
    // Loc bypasses the per-observer `workspace` field — tokei is
    // pointed at the workspace dir directly so it walks only that
    // sub-tree. `.gitignore` resolution still climbs to project root
    // as before via `WalkBuilder::require_git(false)`.
    let loc_root = workspace.unwrap_or(project);
    let loc = if want(MetricKind::Loc) {
        LocObserver::from_config(cfg).scan(loc_root)
    } else {
        LocReport::default()
    };
    let complexity_observer = ComplexityObserver::from_config(cfg).with_workspace(ws_buf.clone());
    let complexity = if want(MetricKind::Complexity) {
        complexity_observer.scan(project)
    } else {
        ComplexityReport::default()
    };
    let churn = (want(MetricKind::Churn) && cfg.metrics.churn.enabled).then(|| {
        ChurnObserver::from_config(cfg)
            .with_workspace(ws_buf.clone())
            .scan(project)
    });
    let change_coupling = (want(MetricKind::ChangeCoupling) && cfg.metrics.change_coupling.enabled)
        .then(|| {
            ChangeCouplingObserver::from_config(cfg)
                .with_workspace(ws_buf.clone())
                .scan(project)
        })
        .map(|mut report| {
            crate::observer::change_coupling::classify_and_filter(
                &mut report,
                loc.primary.as_deref(),
            );
            report
        });
    let duplication =
        (want(MetricKind::Duplication) && cfg.metrics.duplication.enabled).then(|| {
            DuplicationObserver::from_config(cfg)
                .with_workspace(ws_buf.clone())
                .scan(project)
        });
    let hotspot = match (
        want(MetricKind::Hotspot) && cfg.metrics.hotspot.enabled,
        churn.as_ref(),
    ) {
        (true, Some(ch)) => Some(compose_hotspot(
            ch,
            &complexity,
            HotspotWeights {
                churn: cfg.metrics.hotspot.weight_churn,
                complexity: cfg.metrics.hotspot.weight_complexity,
            },
        )),
        _ => None,
    };
    let lcom = (want(MetricKind::Lcom) && cfg.metrics.lcom.enabled).then(|| {
        LcomObserver::from_config(cfg)
            .with_workspace(ws_buf)
            .scan(project)
    });
    ObserverReports {
        loc,
        complexity,
        complexity_observer,
        churn,
        change_coupling,
        duplication,
        hotspot,
        lcom,
    }
}

/// Build a Calibration from a fresh scan's reports. Distribution
/// inputs are per-finding-eligible measurements (per-function CCN /
/// Cognitive, per-file duplicate%, per-pair coupling count, per-file
/// hotspot score). Metrics whose observer didn't run, or whose
/// distribution is empty, omit their entry — `Calibration::classify`
/// then short-circuits to `Severity::Ok` for missing metrics.
///
/// Caller supplies `&Config` so disabled metrics skip calibration even
/// when the underlying complexity scan still produced raw values
/// (CCN/Cognitive share parsing, so both arrive together).
///
/// When `[[project.workspaces]]` is non-empty, the global
/// `Calibration::calibration` table holds breaks for files **outside**
/// every declared workspace (the leftover cohort) and a per-workspace
/// table is added under `Calibration::workspaces.<path>`. With no
/// workspaces declared, the global table holds the whole-repo
/// distribution exactly as before.
pub(crate) fn build_calibration(
    project: &Path,
    reports: &ObserverReports,
    config: &Config,
) -> Calibration {
    let workspaces = &config.project.workspaces;

    let global_filter = |file: &Path| -> bool {
        // No workspaces → whole repo is one cohort.
        // With workspaces → only count files outside every declared one.
        workspaces.is_empty() || crate::core::config::assign_workspace(file, workspaces).is_none()
    };
    let global_metrics = build_metric_calibrations(reports, config, &global_filter);

    let mut workspace_metrics: BTreeMap<String, MetricCalibrations> = BTreeMap::new();
    for ws in workspaces {
        let ws_path = ws.path.trim_end_matches('/').to_string();
        let in_workspace = |file: &Path| -> bool {
            crate::core::config::assign_workspace(file, workspaces).is_some_and(|w| w == ws_path)
        };
        let table = build_metric_calibrations(reports, config, &in_workspace);
        if has_any_table(&table) {
            workspace_metrics.insert(ws_path, table);
        }
    }

    let codebase_files = u32::try_from(
        reports
            .complexity
            .totals
            .files
            .max(reports.loc.total_files()),
    )
    .unwrap_or(u32::MAX);

    Calibration {
        meta: CalibrationMeta {
            created_at: chrono::Utc::now(),
            codebase_files,
            strategy: STRATEGY_PERCENTILE.to_owned(),
            calibrated_at_sha: crate::observer::git::head_sha(project),
        },
        calibration: global_metrics,
        workspaces: workspace_metrics,
    }
    .with_overrides(config)
}

/// Produce a [`MetricCalibrations`] table from the subset of `reports`
/// values whose owning file passes `file_filter`. The same logic backs
/// both the global cohort and each per-workspace cohort — only the
/// filter changes.
fn build_metric_calibrations(
    reports: &ObserverReports,
    config: &Config,
    file_filter: &dyn Fn(&Path) -> bool,
) -> MetricCalibrations {
    let ccn_floors = MetricFloors {
        critical: Some(FLOOR_CCN),
        ok: Some(FLOOR_OK_CCN),
    };
    let cognitive_floors = MetricFloors {
        critical: Some(FLOOR_COGNITIVE),
        ok: Some(FLOOR_OK_COGNITIVE),
    };
    let duplication_floors = MetricFloors {
        critical: Some(FLOOR_DUPLICATION_PCT),
        ok: None,
    };

    let ccn = if config.metrics.ccn.enabled {
        let values: Vec<f64> = reports
            .complexity
            .files
            .iter()
            .filter(|f| file_filter(&f.path))
            .flat_map(|f| f.functions.iter().map(|fun| f64::from(fun.ccn)))
            .collect();
        non_empty(&values).then(|| MetricCalibration::from_distribution(&values, ccn_floors))
    } else {
        None
    };

    let cognitive = if config.metrics.cognitive.enabled {
        let values: Vec<f64> = reports
            .complexity
            .files
            .iter()
            .filter(|f| file_filter(&f.path))
            .flat_map(|f| f.functions.iter().map(|fun| f64::from(fun.cognitive)))
            .collect();
        non_empty(&values).then(|| MetricCalibration::from_distribution(&values, cognitive_floors))
    } else {
        None
    };

    let duplication = reports.duplication.as_ref().and_then(|d| {
        let values: Vec<f64> = d
            .files
            .iter()
            .filter(|f| file_filter(&f.path))
            .map(|f| f.duplicate_pct)
            .collect();
        non_empty(&values)
            .then(|| MetricCalibration::from_distribution(&values, duplication_floors))
    });

    // change_coupling: `min_coupling` already gates pairs at scan time so
    // the absolute floor is rare in practice. lcom: `min_cluster_count`
    // serves the same role. A pair contributes to a cohort iff **both**
    // files pass the filter — cross-cohort pairs are out of scope here
    // (PR4 surfaces them in their own Advisory bucket).
    let change_coupling = reports.change_coupling.as_ref().and_then(|c| {
        let values: Vec<f64> = c
            .pairs
            .iter()
            .filter(|p| file_filter(&p.a) && file_filter(&p.b))
            .map(|p| f64::from(p.count))
            .collect();
        non_empty(&values)
            .then(|| MetricCalibration::from_distribution(&values, MetricFloors::default()))
    });

    let hotspot = reports.hotspot.as_ref().and_then(|h| {
        let scores: Vec<f64> = h
            .entries
            .iter()
            .filter(|e| file_filter(&e.path))
            .map(|e| e.score)
            .collect();
        non_empty(&scores).then(|| HotspotCalibration::from_distribution(&scores))
    });

    let lcom = reports.lcom.as_ref().and_then(|l| {
        let values: Vec<f64> = l
            .classes
            .iter()
            .filter(|c| file_filter(&c.file))
            .map(|c| f64::from(c.cluster_count))
            .collect();
        non_empty(&values)
            .then(|| MetricCalibration::from_distribution(&values, MetricFloors::default()))
    });

    MetricCalibrations {
        ccn,
        cognitive,
        duplication,
        change_coupling,
        hotspot,
        lcom,
    }
}

fn has_any_table(m: &MetricCalibrations) -> bool {
    m.ccn.is_some()
        || m.cognitive.is_some()
        || m.duplication.is_some()
        || m.change_coupling.is_some()
        || m.hotspot.is_some()
        || m.lcom.is_some()
}

/// Compose every enabled Feature's lowered Findings into one Vec,
/// with Severity and the per-file hotspot flag attached. Thin wrapper
/// around [`crate::feature::FeatureRegistry::lower_all`] — the
/// per-metric branches that used to live inline have moved into
/// per-Feature `lower()` impls under `crate::observer::*`.
///
/// `cfg` drives per-Feature `enabled` checks. Calibration entries that
/// are missing (`cal.calibration.<m>` is `None`) fall back to
/// `Severity::Ok`; the `hotspot` flag is `false` whenever the project
/// has no hotspot calibration.
pub(crate) fn classify(reports: &ObserverReports, cal: &Calibration, cfg: &Config) -> Vec<Finding> {
    let mut findings = crate::feature::FeatureRegistry::builtin().lower_all(reports, cfg, cal);
    let workspaces = &cfg.project.workspaces;
    if !workspaces.is_empty() {
        for f in &mut findings {
            f.workspace = crate::core::config::assign_workspace(&f.location.file, workspaces)
                .map(str::to_owned);
        }
    }
    findings
}

/// Run every observer over `scan_root`, classify against the
/// calibration on disk at `paths`, and pack the result into a fresh
/// `FindingsRecord`. Does not write anything — callers decide whether to
/// persist the result via `findings_cache::write_record`.
///
/// Used by both `heal status` (`scan_root` = project, sha/clean from
/// git) and `heal diff` (`scan_root` = transient `git worktree`, sha
/// = the requested ref, clean = true by construction).
pub(crate) fn build_record(
    scan_root: &Path,
    paths: &crate::core::HealPaths,
    cfg: &Config,
    head_sha: Option<String>,
    worktree_clean: bool,
) -> crate::core::findings_cache::FindingsRecord {
    let calibration = Calibration::load(&paths.calibration())
        .ok()
        .map(|c| c.with_overrides(cfg));
    let owned;
    let cal_ref = if let Some(c) = calibration.as_ref() {
        c
    } else {
        owned = Calibration::default();
        &owned
    };
    let reports = run_all(scan_root, cfg, None, None);
    let findings = classify(&reports, cal_ref, cfg);
    let config_hash =
        crate::core::findings_cache::config_hash_from_paths(&paths.config(), &paths.calibration());
    crate::core::findings_cache::FindingsRecord::new(
        head_sha,
        worktree_clean,
        config_hash,
        findings,
    )
}

fn non_empty(values: &[f64]) -> bool {
    !values.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::calibration::{CalibrationMeta, MetricCalibrations, STRATEGY_PERCENTILE};
    use crate::core::finding::IntoFindings;
    use crate::core::severity::Severity;
    use crate::core::severity::SeverityCounts;
    use crate::observer::change_coupling::{ChangeCouplingReport, CouplingTotals, FilePair};
    use crate::observer::complexity::{
        ComplexityObserver, ComplexityReport, ComplexityTotals, FileComplexity, FunctionMetric,
    };
    use crate::observer::duplication::{
        DuplicateBlock, DuplicateLocation, DuplicationReport, DuplicationTotals, FileDuplication,
    };
    use crate::observer::hotspot::{HotspotEntry, HotspotReport, HotspotTotals};
    use crate::observer::loc::LocReport;
    use std::collections::HashSet;
    use std::path::PathBuf;

    /// Synthesize a small but representative `ObserverReports` +
    /// `Calibration` pair. The numbers are picked so each metric has at
    /// least one non-Ok finding under the calibration's breaks.
    #[allow(clippy::too_many_lines)] // shared test fixture; readability > brevity
    fn fixture() -> (ObserverReports, Calibration) {
        let complexity = ComplexityReport {
            files: vec![FileComplexity {
                path: PathBuf::from("src/hot.rs"),
                language: "rust".into(),
                functions: vec![
                    FunctionMetric {
                        name: "tangled".into(),
                        start_line: 10,
                        end_line: 80,
                        ccn: 30,
                        cognitive: 60,
                    },
                    FunctionMetric {
                        name: "tidy".into(),
                        start_line: 100,
                        end_line: 110,
                        ccn: 2,
                        cognitive: 1,
                    },
                ],
            }],
            totals: ComplexityTotals {
                files: 1,
                functions: 2,
                max_ccn: 30,
                max_cognitive: 60,
            },
        };
        let duplication = DuplicationReport {
            blocks: vec![DuplicateBlock {
                token_count: 80,
                locations: vec![
                    DuplicateLocation {
                        path: PathBuf::from("src/hot.rs"),
                        start_line: 200,
                        end_line: 220,
                    },
                    DuplicateLocation {
                        path: PathBuf::from("src/cold.rs"),
                        start_line: 5,
                        end_line: 25,
                    },
                ],
            }],
            files: vec![
                FileDuplication {
                    path: PathBuf::from("src/hot.rs"),
                    total_tokens: 200,
                    duplicate_tokens: 80,
                    duplicate_pct: 40.0,
                },
                FileDuplication {
                    path: PathBuf::from("src/cold.rs"),
                    total_tokens: 200,
                    duplicate_tokens: 10,
                    duplicate_pct: 5.0,
                },
            ],
            totals: DuplicationTotals {
                duplicate_blocks: 1,
                duplicate_tokens: 90,
                files_affected: 2,
            },
            min_tokens: 50,
        };
        let change_coupling = ChangeCouplingReport {
            pairs: vec![FilePair {
                a: PathBuf::from("src/cold.rs"),
                b: PathBuf::from("src/hot.rs"),
                count: 12,
                direction: None,
                class: None,
            }],
            file_sums: Vec::new(),
            totals: CouplingTotals {
                pairs: 1,
                files: 2,
                commits_considered: 50,
            },
            since_days: 90,
            min_coupling: 3,
        };
        let hotspot = HotspotReport {
            entries: vec![
                HotspotEntry {
                    path: PathBuf::from("src/hot.rs"),
                    ccn_sum: 32,
                    churn_commits: 20,
                    score: 640.0,
                },
                HotspotEntry {
                    path: PathBuf::from("src/cold.rs"),
                    ccn_sum: 4,
                    churn_commits: 2,
                    score: 8.0,
                },
            ],
            totals: HotspotTotals {
                files: 2,
                max_score: 640.0,
            },
        };

        let reports = ObserverReports {
            loc: LocReport::default(),
            complexity,
            complexity_observer: ComplexityObserver::default(),
            churn: None,
            change_coupling: Some(change_coupling),
            duplication: Some(duplication),
            hotspot: Some(hotspot),
            lcom: None,
        };

        let cal = Calibration {
            meta: CalibrationMeta {
                created_at: chrono::Utc::now(),
                codebase_files: 2,
                strategy: STRATEGY_PERCENTILE.to_owned(),
                calibrated_at_sha: None,
            },
            calibration: MetricCalibrations {
                ccn: Some(MetricCalibration {
                    p50: 1.0,
                    p75: 5.0,
                    p90: 10.0,
                    p95: 20.0,
                    floor_critical: Some(FLOOR_CCN),
                    floor_ok: Some(FLOOR_OK_CCN),
                }),
                cognitive: Some(MetricCalibration {
                    p50: 1.0,
                    p75: 10.0,
                    p90: 30.0,
                    p95: 50.0,
                    floor_critical: Some(FLOOR_COGNITIVE),
                    floor_ok: Some(FLOOR_OK_COGNITIVE),
                }),
                duplication: Some(MetricCalibration {
                    p50: 5.0,
                    p75: 10.0,
                    p90: 20.0,
                    p95: 35.0,
                    floor_critical: Some(FLOOR_DUPLICATION_PCT),
                    floor_ok: None,
                }),
                change_coupling: Some(MetricCalibration {
                    p50: 1.0,
                    p75: 4.0,
                    p90: 8.0,
                    p95: 16.0,
                    floor_critical: None,
                    floor_ok: None,
                }),
                hotspot: Some(HotspotCalibration {
                    p50: 8.0,
                    p75: 20.0,
                    p90: 100.0,
                    p95: 500.0,
                    floor_ok: Some(crate::core::calibration::FLOOR_OK_HOTSPOT),
                }),
                lcom: None,
            },
            workspaces: BTreeMap::new(),
        };

        (reports, cal)
    }

    /// Drift detector — `classify` must produce the **same** id set as
    /// the observers' own `IntoFindings::into_findings()` output. If
    /// this ever fails, the cache layer's `Finding.id` matching breaks
    /// silently across `heal status` runs.
    #[test]
    fn classify_id_set_matches_into_findings() {
        let (reports, cal) = fixture();

        let mut want: HashSet<String> = reports
            .complexity
            .into_findings()
            .into_iter()
            .map(|f| f.id)
            .collect();
        if let Some(d) = reports.duplication.as_ref() {
            want.extend(d.into_findings().into_iter().map(|f| f.id));
        }
        if let Some(c) = reports.change_coupling.as_ref() {
            want.extend(c.into_findings().into_iter().map(|f| f.id));
        }
        if let Some(h) = reports.hotspot.as_ref() {
            want.extend(h.into_findings().into_iter().map(|f| f.id));
        }

        let got: HashSet<String> = classify(&reports, &cal, &Config::default())
            .into_iter()
            .map(|f| f.id)
            .collect();

        assert_eq!(
            got, want,
            "classify must produce the same Finding.id set as IntoFindings",
        );
    }

    #[test]
    fn classify_assigns_severity_per_metric() {
        let (reports, cal) = fixture();
        let findings = classify(&reports, &cal, &Config::default());

        let by_metric_severity = |metric: &str, severity: Severity| {
            findings
                .iter()
                .filter(|f| f.metric == metric && f.severity == severity)
                .count()
        };

        // CCN=30 hits floor_critical (25); CCN=2 is below p75 (5) → Ok.
        assert!(by_metric_severity("ccn", Severity::Critical) >= 1);
        assert!(by_metric_severity("ccn", Severity::Ok) >= 1);
        // Cognitive=60 hits floor (50); Cognitive=1 → Ok.
        assert!(by_metric_severity("cognitive", Severity::Critical) >= 1);
        assert!(by_metric_severity("cognitive", Severity::Ok) >= 1);
        // Duplication block max_pct = 40.0 ≥ floor (30) → Critical.
        assert!(by_metric_severity("duplication", Severity::Critical) >= 1);
        // Coupling pair count=12 ≥ p90 (8) but < p95 (16) → High.
        assert!(by_metric_severity("change_coupling", Severity::High) >= 1);
        // Hotspot Findings carry no Severity (always Ok).
        assert!(findings
            .iter()
            .filter(|f| f.metric == "hotspot")
            .all(|f| f.severity == Severity::Ok));
    }

    #[test]
    fn classify_flags_hotspot_for_files_above_p90() {
        let (reports, cal) = fixture();
        let findings = classify(&reports, &cal, &Config::default());

        // src/hot.rs score=640 ≥ p90=100 → hotspot. src/cold.rs score=8 < p90.
        let hot_path = PathBuf::from("src/hot.rs");
        let cold_path = PathBuf::from("src/cold.rs");

        // Every CCN/Cognitive finding on hot.rs must carry hotspot=true.
        assert!(findings
            .iter()
            .filter(
                |f| matches!(f.metric.as_str(), "ccn" | "cognitive") && f.location.file == hot_path
            )
            .all(|f| f.hotspot));

        // The duplication block spans hot.rs + cold.rs — even with
        // primary on hot.rs, the per-locations check should flip
        // hotspot=true.
        assert!(findings
            .iter()
            .filter(|f| f.metric == "duplication")
            .all(|f| f.hotspot));

        // The coupling pair touches hot.rs via `locations`. Pair primary
        // is `cold.rs` (lex-smaller) — verify the secondary check fires.
        let pair = findings
            .iter()
            .find(|f| f.metric == "change_coupling")
            .expect("coupling finding present");
        assert_eq!(pair.location.file, cold_path);
        assert!(
            pair.hotspot,
            "coupling hotspot should consider partner file"
        );
    }

    #[test]
    fn classify_with_missing_calibration_falls_back_to_ok() {
        let (reports, _) = fixture();
        let bare = Calibration {
            meta: CalibrationMeta {
                created_at: chrono::Utc::now(),
                codebase_files: 0,
                strategy: STRATEGY_PERCENTILE.to_owned(),
                calibrated_at_sha: None,
            },
            calibration: MetricCalibrations::default(),
            workspaces: BTreeMap::new(),
        };
        let findings = classify(&reports, &bare, &Config::default());
        assert!(
            findings.iter().all(|f| f.severity == Severity::Ok),
            "without calibration, every Finding must be Severity::Ok",
        );
        assert!(
            findings.iter().all(|f| !f.hotspot),
            "without hotspot calibration, the flag must stay false",
        );
    }

    #[test]
    fn severity_counts_from_findings_matches_classify_count() {
        let (reports, cal) = fixture();
        let findings = classify(&reports, &cal, &Config::default());
        let counts = SeverityCounts::from_findings(&findings);

        let total_classified = counts.critical + counts.high + counts.medium + counts.ok;
        assert_eq!(
            total_classified as usize,
            findings.len(),
            "tally must equal Finding count produced by classify",
        );
    }
}
