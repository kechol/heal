//! Docs-feature `heal metrics` sections, gated behind `[features.docs]`.

mod coverage;
mod drift;
mod freshness;
mod hotspot;
mod link_health;
mod orphan_pages;
mod todo_density;

use super::section::MetricSection;

pub(super) fn sections() -> Vec<Box<dyn MetricSection>> {
    vec![
        Box::new(freshness::DocFreshnessSection),
        Box::new(drift::DocDriftSection),
        Box::new(coverage::DocCoverageSection),
        Box::new(link_health::DocLinkHealthSection),
        Box::new(orphan_pages::OrphanPagesSection),
        Box::new(todo_density::TodoDensitySection),
        Box::new(hotspot::DocHotspotSection),
    ]
}
