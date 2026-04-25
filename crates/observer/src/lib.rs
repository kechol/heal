//! HEAL observer crate — metric collection (LOC, AST complexity, churn,
//! duplication, doc skew). v0.1 foundation only ships the trait surface;
//! concrete observers land in subsequent TODO items.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservationMeta {
    pub name: &'static str,
    pub version: u32,
}

/// Marker trait for metric observers. Each observer reads the project tree
/// (and possibly git history) and produces a structured payload to be
/// appended to `.heal/history/*.jsonl`.
pub trait Observer {
    type Output: Serialize;

    fn meta(&self) -> ObservationMeta;
    fn observe(&self, project_root: &std::path::Path) -> anyhow::Result<Self::Output>;
}
