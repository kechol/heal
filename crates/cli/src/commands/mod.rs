pub mod calibrate;
pub mod diff;
pub mod hook;
pub mod init;
pub mod mark_fixed;
pub mod metrics;
pub mod skills;
pub mod status;

/// Print a `Serialize`able payload as pretty-printed JSON to stdout.
/// Used by every `--json` handler — owned data is infallible to
/// serialise, so the `expect` is structurally true.
pub(crate) fn emit_json<T: serde::Serialize>(value: &T) {
    let body = serde_json::to_string_pretty(value).expect("serialization is infallible");
    println!("{body}");
}
