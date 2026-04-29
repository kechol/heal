//! Internal library for the `heal` CLI binary.
//!
//! These modules used to live in separate `heal-core` and `heal-observer`
//! crates. They were inlined into `heal-cli` so the workspace ships as a
//! single crate on crates.io — `cargo install heal-cli` is the supported
//! installation path. The module shape is preserved (`crate::core::*`,
//! `crate::observer::*`) so call sites read the same as before.
//!
//! The exposed surface here is **unstable**. Treat it as the implementation
//! detail of the `heal` binary; no semver guarantees apply outside the CLI
//! contract documented in `README.md`.

#![doc(hidden)]
// The internal API is `pub` only so integration tests under `tests/` can
// reach it. Clippy's `pedantic` would otherwise demand `#[must_use]` on
// every accessor — noise for an unstable internal surface.
#![allow(clippy::must_use_candidate)]

pub mod cli;
pub mod commands;
pub mod core;
pub mod finding;
pub mod observer;
pub mod observers;
pub mod plugin_assets;
pub mod snapshot;
#[cfg(test)]
mod test_support;
