//! Tiny terminal-rendering helpers shared by every CLI command that
//! emits coloured output. Centralised so the four ANSI SGR constants
//! and the `is_terminal()`-aware `ansi_wrap` only live in one place.

/// ANSI SGR colour codes. `pub` so the small set of CLI commands
/// that emit colour can share the constants instead of redefining them.
pub const ANSI_RED: &str = "\x1b[31m";
pub const ANSI_GREEN: &str = "\x1b[32m";
pub const ANSI_YELLOW: &str = "\x1b[33m";
pub const ANSI_CYAN: &str = "\x1b[36m";

/// Wrap `text` in `color` (one of `ANSI_*`) followed by the SGR reset
/// when `enabled`; otherwise return `text` unchanged. Centralises the
/// `is_terminal()` gating that every colorising call site does.
#[must_use]
pub fn ansi_wrap(color: &str, text: &str, enabled: bool) -> String {
    if enabled {
        format!("{color}{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}
