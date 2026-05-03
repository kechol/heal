//! HEAL core: shared types, config, event-log rotation, and the
//! persisted Calibration schema.

pub mod calibration;
pub mod check_cache;
pub mod compaction;
pub mod config;
pub mod error;
pub mod eventlog;
pub mod finding;
pub mod fs;
pub mod hash;
pub mod monorepo;
pub mod paths;
pub mod severity;
pub mod term;

pub use error::{Error, Result};
pub use paths::HealPaths;
