//! Code-feature observer family. Reads project source + git history and
//! emits the v0.x code-substrate Findings. Per-metric on/off lives under
//! `[metrics.<m>]` in `.heal/config.toml`.

pub mod change_coupling;
pub mod churn;
pub mod complexity;
pub mod duplication;
pub mod hotspot;
pub mod lcom;
pub mod loc;
