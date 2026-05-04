//! Test-feature `heal metrics` sections, gated behind `[features.test]`.

mod coverage;

use super::section::MetricSection;

pub(super) fn sections() -> Vec<Box<dyn MetricSection>> {
    vec![Box::new(coverage::CoveragePctSection)]
}
