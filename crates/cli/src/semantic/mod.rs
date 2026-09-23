//! `[features.semantic]`: opt-in judgments from `TypeSafe`'s Jev classifier.
//!
//! Division of labour. HEAL's observers decide **which** code, tests, or
//! docs are worth asking about (hotspots, pairs, clusters); a task turns
//! each into one typed question; Jev answers with a probability; the
//! answer is cached under `.heal/semantic/verdicts/`; and `Feature::lower`
//! reads the cache offline. The agent (skills) keeps everything
//! generative — names, prose, patches.
//!
//! Network boundary. [`client`] is used only by `heal semantic ask` and by
//! the key check in `heal auth jev status`. Every other command is offline.
//! Prior art: mizchi/jev-lint and mizchi/jev-lexer (MIT) for the client's
//! retry/pacing behaviour and the verdict-cache design.

pub mod api;
pub mod client;
pub mod cost;
pub mod credentials;
pub mod pacer;
pub mod plan;
pub mod runner;
pub mod store;
pub mod task;
