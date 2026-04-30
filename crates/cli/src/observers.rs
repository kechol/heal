//! Single source of truth for "run every enabled observer". Both `heal
//! status` and the post-commit snapshot writer call this so a new
//! observer or enable-flag only needs editing in one place.

use std::path::Path;

use crate::core::calibration::{
    Calibration, CalibrationMeta, HotspotCalibration, MetricCalibration, MetricCalibrations,
    FLOOR_CCN, FLOOR_COGNITIVE, FLOOR_DUPLICATION_PCT, STRATEGY_PERCENTILE,
};
use crate::core::config::Config;
use crate::core::severity::Severity;
use crate::core::snapshot::SeverityCounts;
use crate::observer::change_coupling::{ChangeCouplingObserver, ChangeCouplingReport};
use crate::observer::churn::{ChurnObserver, ChurnReport};
use crate::observer::complexity::{ComplexityObserver, ComplexityReport};
use crate::observer::duplication::{DuplicationObserver, DuplicationReport};
use crate::observer::hotspot::{compose as compose_hotspot, HotspotReport, HotspotWeights};
use crate::observer::loc::{LocObserver, LocReport};

use crate::cli::StatusMetric;

pub(crate) struct ObserverReports {
    pub loc: LocReport,
    pub complexity: ComplexityReport,
    pub complexity_observer: ComplexityObserver,
    pub churn: Option<ChurnReport>,
    pub change_coupling: Option<ChangeCouplingReport>,
    pub duplication: Option<DuplicationReport>,
    pub hotspot: Option<HotspotReport>,
}

/// Run the observers needed for the requested metric. `only = None`
/// means "run everything" (snapshot capture for the commit hook). When
/// `only` is set, observers irrelevant to that metric are skipped —
/// churn and complexity still run for `Hotspot` because the composite
/// is built on top of them. The skipped observers' fields fall back to
/// `Default` (or `None` for the optional ones).
pub(crate) fn run_all(project: &Path, cfg: &Config, only: Option<StatusMetric>) -> ObserverReports {
    let want = |m: StatusMetric| match only {
        None => true,
        Some(o) if o == m => true,
        Some(StatusMetric::Hotspot)
            if matches!(m, StatusMetric::Churn | StatusMetric::Complexity) =>
        {
            true
        }
        _ => false,
    };

    let loc = if want(StatusMetric::Loc) {
        LocObserver::from_config(cfg).scan(project)
    } else {
        LocReport::default()
    };
    let complexity_observer = ComplexityObserver::from_config(cfg);
    let complexity = if want(StatusMetric::Complexity) {
        complexity_observer.scan(project)
    } else {
        ComplexityReport::default()
    };
    let churn = (want(StatusMetric::Churn) && cfg.metrics.churn.enabled)
        .then(|| ChurnObserver::from_config(cfg).scan(project));
    let change_coupling = (want(StatusMetric::ChangeCoupling)
        && cfg.metrics.change_coupling.enabled)
        .then(|| ChangeCouplingObserver::from_config(cfg).scan(project));
    let duplication = (want(StatusMetric::Duplication) && cfg.metrics.duplication.enabled)
        .then(|| DuplicationObserver::from_config(cfg).scan(project));
    let hotspot = match (
        want(StatusMetric::Hotspot) && cfg.metrics.hotspot.enabled,
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
    ObserverReports {
        loc,
        complexity,
        complexity_observer,
        churn,
        change_coupling,
        duplication,
        hotspot,
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
pub(crate) fn build_calibration(reports: &ObserverReports, config: &Config) -> Calibration {
    let ccn = if config.metrics.ccn.enabled {
        let values: Vec<f64> = reports
            .complexity
            .files
            .iter()
            .flat_map(|f| f.functions.iter().map(|fun| f64::from(fun.ccn)))
            .collect();
        non_empty(&values).then(|| MetricCalibration::from_distribution(&values, Some(FLOOR_CCN)))
    } else {
        None
    };

    let cognitive = if config.metrics.cognitive.enabled {
        let values: Vec<f64> = reports
            .complexity
            .files
            .iter()
            .flat_map(|f| f.functions.iter().map(|fun| f64::from(fun.cognitive)))
            .collect();
        non_empty(&values)
            .then(|| MetricCalibration::from_distribution(&values, Some(FLOOR_COGNITIVE)))
    } else {
        None
    };

    let duplication = reports.duplication.as_ref().and_then(|d| {
        let values: Vec<f64> = d.files.iter().map(|f| f.duplicate_pct).collect();
        non_empty(&values)
            .then(|| MetricCalibration::from_distribution(&values, Some(FLOOR_DUPLICATION_PCT)))
    });

    let change_coupling = reports.change_coupling.as_ref().and_then(|c| {
        let values: Vec<f64> = c.pairs.iter().map(|p| f64::from(p.count)).collect();
        // `min_coupling` already gates coupling pairs at scan time so
        // the absolute floor here is rare in practice — leave None.
        non_empty(&values).then(|| MetricCalibration::from_distribution(&values, None))
    });

    let hotspot = reports.hotspot.as_ref().and_then(|h| {
        let scores: Vec<f64> = h.entries.iter().map(|e| e.score).collect();
        non_empty(&scores).then(|| HotspotCalibration::from_distribution(&scores))
    });

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
        },
        calibration: MetricCalibrations {
            ccn,
            cognitive,
            duplication,
            change_coupling,
            hotspot,
        },
    }
    .with_overrides(config)
}

/// Walk the observer reports with an applied calibration and tally
/// the resulting Severity per finding. Symmetrical to the iteration
/// inside `IntoFindings` impls — every finding the observers would
/// emit gets one tally entry. Metrics without a calibration fall
/// through silently (those findings are uncalibrated and tallied as
/// `Severity::Ok`).
pub(crate) fn tally_severity(reports: &ObserverReports, cal: &Calibration) -> SeverityCounts {
    let mut counts = SeverityCounts::default();

    for file in &reports.complexity.files {
        for fun in &file.functions {
            if fun.ccn > 0 {
                let s = cal
                    .calibration
                    .ccn
                    .as_ref()
                    .map_or(Severity::default(), |c| c.classify(f64::from(fun.ccn)));
                counts.tally(s);
            }
            if fun.cognitive > 0 {
                let s = cal
                    .calibration
                    .cognitive
                    .as_ref()
                    .map_or(Severity::default(), |c| {
                        c.classify(f64::from(fun.cognitive))
                    });
                counts.tally(s);
            }
        }
    }

    if let Some(d) = reports.duplication.as_ref() {
        let cal_dup = cal.calibration.duplication.as_ref();
        for f in &d.files {
            // Only emit a finding when there's *some* duplication to
            // discuss. Files with 0% duplication shouldn't show up as
            // Ok-rated findings — they have nothing to fix.
            if f.duplicate_tokens == 0 {
                continue;
            }
            let s = cal_dup.map_or(Severity::default(), |c| c.classify(f.duplicate_pct));
            counts.tally(s);
        }
    }

    if let Some(c) = reports.change_coupling.as_ref() {
        let cal_cc = cal.calibration.change_coupling.as_ref();
        for p in &c.pairs {
            let s = cal_cc.map_or(Severity::default(), |cc| cc.classify(f64::from(p.count)));
            counts.tally(s);
        }
    }

    counts
}

fn non_empty(values: &[f64]) -> bool {
    !values.is_empty()
}
