//! HEAL observer crate — metric collection (LOC, AST complexity, churn,
//! duplication, doc skew). v0.1 foundation only ships the trait surface;
//! concrete observers land in subsequent TODO items.

use serde::{Deserialize, Serialize};

pub mod change_coupling;
pub mod churn;
pub mod complexity;
pub mod duplication;
pub mod git;
pub mod hotspot;
pub mod lang;
pub mod lcom;
pub mod loc;
mod walk;

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

/// Generates the `with_workspace(self, ws: Option<PathBuf>) -> Self`
/// builder for an observer that carries a `workspace: Option<PathBuf>`
/// field. Centralised so the five observers honoring `--workspace`
/// (`ComplexityObserver`, `LcomObserver`, `DuplicationObserver`,
/// `ChurnObserver`, `ChangeCouplingObserver`) don't carry five
/// verbatim copies of the same setter.
macro_rules! impl_workspace_builder {
    ($t:ty) => {
        impl $t {
            #[must_use]
            pub fn with_workspace(mut self, workspace: Option<std::path::PathBuf>) -> Self {
                self.workspace = workspace;
                self
            }
        }
    };
}
pub(crate) use impl_workspace_builder;
