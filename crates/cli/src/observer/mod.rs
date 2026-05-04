//! Observer families and the trait surface they share.
//!
//! Findings-emitting observers live under [`code`] and [`docs`];
//! [`shared`] holds Findings-free utilities consumed by both families.

use serde::{Deserialize, Serialize};

pub mod code;
pub mod docs;
pub mod shared;
pub mod test;

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
/// field. Centralized so the five observers honoring `--workspace`
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
