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

pub(crate) struct ObserverReports {
    pub loc: LocReport,
    pub complexity: ComplexityReport,
    pub complexity_observer: ComplexityObserver,
    pub churn: Option<ChurnReport>,
    pub change_coupling: Option<ChangeCouplingReport>,
    pub duplication: Option<DuplicationReport>,
    pub hotspot: Option<HotspotReport>,
}

pub(crate) fn run_all(project: &Path, cfg: &Config) -> ObserverReports {
    let loc = LocObserver::from_config(cfg).scan(project);
    let complexity_observer = ComplexityObserver::from_config(cfg);
    let complexity = complexity_observer.scan(project);
    let churn = cfg
        .metrics
        .churn
        .enabled
        .then(|| ChurnObserver::from_config(cfg).scan(project));
    let change_coupling = cfg
        .metrics
        .change_coupling
        .enabled
        .then(|| ChangeCouplingObserver::from_config(cfg).scan(project));
    let duplication = cfg
        .metrics
        .duplication
        .enabled
        .then(|| DuplicationObserver::from_config(cfg).scan(project));
    let hotspot = match (cfg.metrics.hotspot.enabled, churn.as_ref()) {
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
