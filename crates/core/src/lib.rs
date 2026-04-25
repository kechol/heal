//! HEAL core: shared types, config, history rotation, and state management.

pub mod config;
pub mod error;
pub mod history;
pub mod paths;
pub mod state;

pub use error::{Error, Result};
pub use paths::HealPaths;
