//! Test-feature observer family. Gated behind `[features.test]`. Reads
//! externally-generated lcov files (`cargo llvm-cov`, `pytest-cov`,
//! `nyc`, `scoverage`) and surfaces low-coverage hotspots; the
//! `is_test_file` flag-tagging plumbing also lives here so the doc /
//! code observer families stay test-oblivious.
//!
//! HEAL never executes tests. Generation of the lcov file is the
//! user's contract — the binary is a read-only consumer.

pub mod coverage;
pub mod hotspot;
pub mod lcov;
pub mod skip_ratio;
