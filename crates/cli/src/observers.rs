//! Single source of truth for "run every enabled observer". Both `heal
//! status` and the post-commit snapshot writer call this so a new
//! observer or enable-flag only needs editing in one place.

use std::path::Path;

use heal_core::config::Config;
use heal_observer::change_coupling::{ChangeCouplingObserver, ChangeCouplingReport};
use heal_observer::churn::{ChurnObserver, ChurnReport};
use heal_observer::complexity::{ComplexityObserver, ComplexityReport};
use heal_observer::duplication::{DuplicationObserver, DuplicationReport};
use heal_observer::hotspot::{compose as compose_hotspot, HotspotReport, HotspotWeights};
use heal_observer::loc::{LocObserver, LocReport};

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
