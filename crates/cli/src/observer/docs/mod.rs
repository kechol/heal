//! Docs-feature observer family. Gated behind `[features.docs]`. Reads
//! Layer A (`.heal/doc_pairs.json`) + Layer B (standalone prose docs)
//! and emits the v0.3+ docs-substrate Findings.
//!
//! [`corpus`], [`walk`], and [`markdown`] are Findings-free helpers
//! shared across the observer set.

pub mod corpus;
pub mod coverage;
pub mod drift;
pub mod freshness;
pub mod link_health;
pub(crate) mod markdown;
pub mod orphan_pages;
pub mod todo_density;
pub mod walk;
