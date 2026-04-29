//! HEAL core: shared types, config, event-log rotation, and state management.

pub mod config;
pub mod error;
pub mod eventlog;
pub mod finding;
pub mod paths;
pub mod snapshot;
pub mod state;

pub use error::{Error, Result};
pub use paths::HealPaths;
