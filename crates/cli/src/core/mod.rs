//! HEAL core: shared types, config, event-log rotation, and the persisted
//! `MetricsSnapshot` schema.

pub mod calibration;
pub mod config;
pub mod error;
pub mod eventlog;
pub mod finding;
pub mod fs;
pub mod hash;
pub mod paths;
pub mod severity;
pub mod snapshot;

pub use error::{Error, Result};
pub use paths::HealPaths;
