//! Code-feature `heal metrics` sections. Each child module owns one
//! `MetricSection` impl for a metric defined under `[metrics.<m>]`.

mod change_coupling;
mod churn;
mod complexity;
mod duplication;
mod hotspot;
mod lcom;
mod loc;

use super::section::MetricSection;

pub(super) fn sections() -> Vec<Box<dyn MetricSection>> {
    vec![
        Box::new(loc::LocSection),
        Box::new(complexity::ComplexitySection),
        Box::new(churn::ChurnSection),
        Box::new(change_coupling::ChangeCouplingSection),
        Box::new(duplication::DuplicationSection),
        Box::new(hotspot::HotspotSection),
        Box::new(lcom::LcomSection),
    ]
}
