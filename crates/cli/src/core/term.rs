//! Tiny terminal-rendering helpers shared by every CLI command that
//! emits colored output. Centralized so the four ANSI SGR constants
//! and the `is_terminal()`-aware `ansi_wrap` only live in one place.

use std::io::IsTerminal;
use std::process::{Command, Stdio};

use anyhow::Result;

/// ANSI SGR color codes. `pub` so the small set of CLI commands
/// that emit color can share the constants instead of redefining them.
pub const ANSI_RED: &str = "\x1b[31m";
pub const ANSI_GREEN: &str = "\x1b[32m";
pub const ANSI_YELLOW: &str = "\x1b[33m";
pub const ANSI_CYAN: &str = "\x1b[36m";

/// Wrap `text` in `color` (one of `ANSI_*`) followed by the SGR reset
/// when `enabled`; otherwise return `text` unchanged. Centralizes the
/// `is_terminal()` gating that every colorizing call site does.
#[must_use]
pub fn ansi_wrap(color: &str, text: &str, enabled: bool) -> String {
    if enabled {
        format!("{color}{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}

/// Render `produce` either through `$PAGER` (default `less`) when stdout
/// is a TTY and `no_pager` is false, or directly to stdout otherwise.
/// `produce(out, colorize)` writes to the provided sink; `colorize` is
/// `true` when the destination is a terminal or the pager (which passes
/// ANSI through via `LESS=FRX`). Broken pipes (user quit `less` early)
/// are swallowed so the process still exits 0.
pub fn write_through_pager<F>(no_pager: bool, produce: F) -> Result<()>
where
    F: FnOnce(&mut dyn std::io::Write, bool) -> Result<()>,
{
    let stdout_is_tty = std::io::stdout().is_terminal();
    if !no_pager && stdout_is_tty {
        if let Some(mut child) = spawn_pager() {
            // Pager renders ANSI colors via `LESS=R`, so always colorize.
            let stdin = child.stdin.take().expect("stdin was piped");
            let mut buf = std::io::BufWriter::new(stdin);
            let render_result = produce(&mut buf, true);
            // Drop buf to close the pipe so `less` can finish; then wait.
            drop(buf);
            child.wait().ok();
            return swallow_broken_pipe(render_result);
        }
        // Pager spawn failed (e.g. `less` not installed) — fall through.
    }
    let mut stdout = std::io::stdout();
    let colorize = stdout_is_tty;
    swallow_broken_pipe(produce(&mut stdout, colorize))
}

/// Spawn `$PAGER` (or `less`) with a TTY-friendly default `LESS` env so
/// short output exits without taking over the screen and ANSI colors
/// pass through. Returns `None` when no pager binary is available.
fn spawn_pager() -> Option<std::process::Child> {
    let pager = std::env::var_os("PAGER").unwrap_or_else(|| "less".into());
    let mut cmd = Command::new(&pager);
    cmd.stdin(Stdio::piped());
    if std::env::var_os("LESS").is_none() {
        // F: quit if the output fits on one screen.
        // R: pass ANSI control chars through unchanged.
        // X: don't send the alt-screen init/deinit, so output stays
        //    visible after the pager exits.
        cmd.env("LESS", "FRX");
    }
    cmd.spawn().ok()
}

/// Treat a `BrokenPipe` (the user quit `less` before all output was
/// written) as a normal exit — every other I/O error still propagates.
fn swallow_broken_pipe(result: Result<()>) -> Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(err) => {
            let is_broken_pipe = err
                .chain()
                .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
                .any(|io| io.kind() == std::io::ErrorKind::BrokenPipe);
            if is_broken_pipe {
                Ok(())
            } else {
                Err(err)
            }
        }
    }
}
