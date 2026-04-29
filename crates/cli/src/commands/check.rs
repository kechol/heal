//! `heal check [SKILL]` — launch Claude Code (`claude -p`) on a
//! `check-*` skill.
//!
//! By default, `claude -p` is invoked with `--output-format stream-json`
//! and the events are parsed into a per-tool progress feed on stderr
//! (`[ 1.2s] → Bash heal status --metric hotspot --json`) so the user
//! sees what's happening instead of waiting silently. The final
//! synthesis lands on stdout. `--quiet` suppresses progress; `--raw`
//! skips parsing entirely and forwards claude's output verbatim.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::cli::{CheckArgs, CheckSkill, OutputFormat};

const PLUGIN_ROOT_REL: &str = ".claude/plugins/heal";
const SKILLS_DIR_REL: &str = ".claude/plugins/heal/skills";

pub fn run(project: &Path, args: &CheckArgs) -> Result<()> {
    ensure_plugin_installed(project, args.skill)?;

    let cfg = heal_core::config::load_from_project(project).ok();
    let language = cfg
        .as_ref()
        .and_then(|c| c.project.response_language.as_deref());
    let resolved_format = resolve_format(args.format, std::io::stdout().is_terminal());
    let prompt = build_prompt(args.skill, language, resolved_format);
    let mut cmd = Command::new("claude");
    cmd.arg("-p").arg(&prompt);

    let parse_stream = !args.raw && !user_overrides_output_format(&args.claude_args);
    if parse_stream {
        cmd.arg("--output-format")
            .arg("stream-json")
            .arg("--verbose");
    }
    for a in &args.claude_args {
        cmd.arg(a);
    }
    cmd.current_dir(project);

    if parse_stream {
        run_with_progress(cmd, args.skill, args.quiet)
    } else {
        run_passthrough(cmd)
    }
}

fn ensure_plugin_installed(project: &Path, skill: CheckSkill) -> Result<()> {
    let plugin_root = project.join(PLUGIN_ROOT_REL);
    let skill_md = project
        .join(SKILLS_DIR_REL)
        .join(skill.skill_name())
        .join("SKILL.md");
    if !plugin_root.is_dir() || !skill_md.is_file() {
        bail!(
            "HEAL plugin not installed at {} (missing {}). Run `heal skills install` first.",
            plugin_root.display(),
            skill_md.display(),
        );
    }
    Ok(())
}

fn run_passthrough(mut cmd: Command) -> Result<()> {
    let status = cmd.status().with_context(spawn_error_hint)?;
    if !status.success() {
        bail!("`claude` exited with {status}");
    }
    Ok(())
}

fn run_with_progress(mut cmd: Command, skill: CheckSkill, quiet: bool) -> Result<()> {
    cmd.stdout(Stdio::piped());
    let mut child = cmd.spawn().with_context(spawn_error_hint)?;
    let started = Instant::now();
    if !quiet {
        eprintln!("→ heal check {} (Ctrl+C to cancel)", skill.short_name());
    }
    let stdout = child
        .stdout
        .take()
        .expect("stdout was piped by run_with_progress");
    let reader = BufReader::new(stdout);
    let mut final_text = String::new();
    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            // Non-JSON: claude isn't streaming after all. Surface the
            // line so the user still sees output instead of a silent hang.
            if !quiet {
                eprintln!("{line}");
            }
            continue;
        };
        handle_event(&event, started, quiet, &mut final_text);
    }
    let status = child.wait()?;
    if !quiet {
        eprintln!("→ done in {:.1}s", started.elapsed().as_secs_f64());
    }
    if !final_text.is_empty() {
        let mut out = std::io::stdout().lock();
        writeln!(out, "{final_text}")?;
    }
    if !status.success() {
        bail!("`claude` exited with {status}");
    }
    Ok(())
}

/// Translate one stream-json event into either a progress line (printed
/// to stderr unless `quiet`) or a chunk of final-answer text appended
/// directly into `final_text` — borrowing the slice from the parsed
/// `Value` avoids a clone of what may be a multi-KB synthesis.
fn handle_event(event: &Value, started: Instant, quiet: bool, final_text: &mut String) {
    let Some(etype) = event.get("type").and_then(Value::as_str) else {
        return;
    };
    match etype {
        "assistant" => {
            let Some(content) = event.pointer("/message/content").and_then(Value::as_array) else {
                return;
            };
            if quiet {
                return;
            }
            for c in content {
                if c.get("type").and_then(Value::as_str) == Some("tool_use") {
                    let name = c.get("name").and_then(Value::as_str).unwrap_or("?");
                    let summary = summarize_tool_input(name, c.get("input"));
                    eprintln!(
                        "  [{:>5.1}s] → {name}{summary}",
                        started.elapsed().as_secs_f64()
                    );
                }
            }
        }
        "result" => {
            // `claude --output-format stream-json` emits exactly one
            // `result` event per session today, but guard against a
            // future protocol that splits it: separate chunks with a
            // newline so `result1result2` can't collide.
            if let Some(text) = event.get("result").and_then(Value::as_str) {
                if !final_text.is_empty() {
                    final_text.push('\n');
                }
                final_text.push_str(text);
            }
        }
        _ => {}
    }
}

/// Render a tool-use input as a one-line summary for the progress feed.
/// Bash gets its `command`, Read gets the file basename, Grep/Glob get
/// their pattern; everything else falls back to the tool name only.
fn summarize_tool_input(name: &str, input: Option<&Value>) -> String {
    let Some(input) = input else {
        return String::new();
    };
    let pick_str = |key: &str| input.get(key).and_then(Value::as_str).unwrap_or("");
    match name {
        "Bash" => format!(" {}", truncate(pick_str("command"), 80)),
        "Read" => format!(" {}", basename(pick_str("file_path"))),
        "Grep" | "Glob" => format!(" {}", truncate(pick_str("pattern"), 60)),
        _ => String::new(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}

fn basename(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
}

fn build_prompt(skill: CheckSkill, language: Option<&str>, format: OutputFormat) -> String {
    let name = skill.skill_name();
    let mut prompt = format!(
        "Run the `{name}` skill (defined at `{SKILLS_DIR_REL}/{name}/SKILL.md`) \
         on this project. Follow the SKILL.md procedure exactly, do not modify any files."
    );
    if let Some(lang) = language {
        let _ = write!(prompt, " Write the response in {lang}.");
    }
    if matches!(format, OutputFormat::Plain) {
        prompt.push_str(
            " Output as plain text suitable for a terminal: \
             no markdown headings, no `**bold**` or `*italic*` markers, \
             no nested bullet trees — use simple indentation and dashes \
             for structure. Inline code identifiers (file paths, function \
             names) may stay in backticks.",
        );
    }
    prompt
}

/// Resolve [`OutputFormat::Auto`] using whether stdout is a terminal.
/// Pipe → keep markdown so downstream renderers (`bat`, `glow`) work
/// as expected; TTY → strip markdown so `**` / `#` don't show up raw.
fn resolve_format(format: OutputFormat, stdout_is_tty: bool) -> OutputFormat {
    match format {
        OutputFormat::Auto => {
            if stdout_is_tty {
                OutputFormat::Plain
            } else {
                OutputFormat::Markdown
            }
        }
        explicit => explicit,
    }
}

/// Detect whether the user already passed `--output-format` (in either
/// the space-separated `--output-format X` or `--output-format=X` form)
/// so HEAL doesn't inject a competing copy and break the `claude` call.
fn user_overrides_output_format(claude_args: &[String]) -> bool {
    claude_args
        .iter()
        .any(|a| a == "--output-format" || a.starts_with("--output-format="))
}

fn spawn_error_hint() -> &'static str {
    "failed to spawn `claude`. Is Claude Code installed and on PATH? \
     Install: https://docs.claude.com/en/docs/claude-code/setup"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn refuses_when_plugin_not_installed() {
        let dir = TempDir::new().unwrap();
        let args = CheckArgs {
            skill: CheckSkill::Overview,
            format: OutputFormat::Auto,
            quiet: false,
            raw: false,
            claude_args: Vec::new(),
        };
        let err = run(dir.path(), &args).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("HEAL plugin not installed"),
            "expected install hint, got: {msg}",
        );
        assert!(msg.contains("heal skills install"));
    }

    #[test]
    fn prompt_references_skill_md_path() {
        let prompt = build_prompt(CheckSkill::Hotspots, None, OutputFormat::Markdown);
        assert!(prompt.contains("check-hotspots"));
        assert!(prompt.contains("SKILL.md"));
        assert!(prompt.contains("do not modify"));
    }

    #[test]
    fn prompt_includes_language_hint_when_set() {
        let prompt = build_prompt(
            CheckSkill::Overview,
            Some("Japanese"),
            OutputFormat::Markdown,
        );
        assert!(prompt.contains("Write the response in Japanese."));
    }

    #[test]
    fn prompt_includes_plain_text_hint_when_format_plain() {
        let prompt = build_prompt(CheckSkill::Overview, None, OutputFormat::Plain);
        assert!(prompt.contains("plain text"));
        assert!(prompt.contains("no markdown"));
    }

    #[test]
    fn prompt_omits_format_hint_for_markdown() {
        let prompt = build_prompt(CheckSkill::Overview, None, OutputFormat::Markdown);
        assert!(!prompt.contains("plain text"));
    }

    #[test]
    fn resolve_format_auto_picks_plain_for_tty() {
        assert_eq!(
            resolve_format(OutputFormat::Auto, true),
            OutputFormat::Plain
        );
        assert_eq!(
            resolve_format(OutputFormat::Auto, false),
            OutputFormat::Markdown
        );
    }

    #[test]
    fn user_override_detects_both_argument_forms() {
        let space = vec!["--output-format".to_string(), "json".to_string()];
        let equals = vec!["--output-format=stream-json".to_string()];
        let unrelated = vec!["--model".to_string(), "haiku".to_string()];
        assert!(user_overrides_output_format(&space));
        assert!(user_overrides_output_format(&equals));
        assert!(!user_overrides_output_format(&unrelated));
        assert!(!user_overrides_output_format(&[]));
    }

    #[test]
    fn resolve_format_explicit_overrides_tty() {
        assert_eq!(
            resolve_format(OutputFormat::Plain, false),
            OutputFormat::Plain
        );
        assert_eq!(
            resolve_format(OutputFormat::Markdown, true),
            OutputFormat::Markdown
        );
    }

    #[test]
    fn summarize_bash_includes_command() {
        let input = json!({ "command": "heal status --metric hotspot --json" });
        let s = summarize_tool_input("Bash", Some(&input));
        assert!(s.contains("heal status --metric hotspot --json"));
    }

    #[test]
    fn summarize_read_uses_basename() {
        let input = json!({ "file_path": "/abs/path/to/foo.rs" });
        let s = summarize_tool_input("Read", Some(&input));
        assert_eq!(s.trim(), "foo.rs");
    }

    #[test]
    fn truncate_appends_ellipsis_when_over_max() {
        assert_eq!(truncate("hello", 10), "hello");
        let long = "abcdefghijklmnop";
        let out = truncate(long, 8);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= 8);
    }

    #[test]
    fn handle_event_writes_result_into_buffer() {
        let event = json!({ "type": "result", "result": "the synthesis" });
        let mut out = String::new();
        handle_event(&event, Instant::now(), true, &mut out);
        assert_eq!(out, "the synthesis");
    }

    #[test]
    fn handle_event_separates_repeated_result_chunks() {
        let mut out = String::new();
        let started = Instant::now();
        handle_event(
            &json!({ "type": "result", "result": "first" }),
            started,
            true,
            &mut out,
        );
        handle_event(
            &json!({ "type": "result", "result": "second" }),
            started,
            true,
            &mut out,
        );
        assert_eq!(out, "first\nsecond");
    }

    #[test]
    fn handle_event_ignores_non_tool_assistant() {
        let event = json!({
            "type": "assistant",
            "message": { "content": [{ "type": "text", "text": "hi" }] }
        });
        let mut out = String::new();
        handle_event(&event, Instant::now(), true, &mut out);
        assert!(out.is_empty());
    }
}
