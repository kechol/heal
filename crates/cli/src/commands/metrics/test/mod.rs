//! Test-feature `heal metrics` sections, gated behind `[features.test]`.

mod coverage;
mod hotspot;
mod skip_ratio;

use super::section::MetricSection;

pub(super) fn sections() -> Vec<Box<dyn MetricSection>> {
    vec![
        Box::new(coverage::CoveragePctSection),
        Box::new(skip_ratio::SkipRatioSection),
        Box::new(hotspot::TestHotspotSection),
    ]
}
