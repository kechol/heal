//! HEAL core: shared types, config, event-log rotation, and the
//! persisted Calibration schema.

pub mod accepted;
pub mod calibration;
pub mod config;
pub mod error;
pub mod finding;
pub mod findings_cache;
pub mod fs;
pub mod hash;
pub mod monorepo;
pub mod paths;
pub mod severity;
pub mod term;

pub use error::{Error, Result};
pub use paths::HealPaths;
