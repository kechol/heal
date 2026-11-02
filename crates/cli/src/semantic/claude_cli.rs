//! Asking Claude through the `claude` CLI, for people signed in to Claude
//! Code who have no API key.
//!
//! The prompt goes over stdin, so project content never appears in a
//! process listing. The child runs from the temp directory with no tools,
//! MCP servers, slash commands, settings, or saved session: the repository
//! text it judges is untrusted, and nothing in it may reach anything but
//! the answer. HEAL opens no connection here; the CLI does.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use crate::semantic::api::{Question, Response, Usage};
use crate::semantic::claude::{
    answer_schema, decode_answers, user_prompt, RawAnswers, EFFORT, SYSTEM_PROMPT,
};
use crate::semantic::client::{
    backoff, truncate, AskError, AskErrorKind, Classifier, ModelCheck, Sleeper, SpendMeter,
};

const PROGRAM: &str = "claude";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);
/// Each request is a whole CLI process, far heavier than an HTTP request.
const MAX_CONCURRENCY: usize = 4;
/// Fewer than over HTTP: every attempt starts a process, and a timed-out
/// one has already waited [`REQUEST_TIMEOUT`].
const RETRIES: u32 = 2;

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
}

/// The process boundary. Tests hand in a fake so no test ever starts `claude`.
pub trait CommandRunner: Send + Sync {
    fn run(&self, args: &[String], stdin: &[u8]) -> Result<CommandOutput, String>;
}

/// Whether `claude` is on `PATH`. Starts nothing.
#[must_use]
pub fn available() -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths)
            .any(|dir| dir.join(PROGRAM).is_file() || dir.join(format!("{PROGRAM}.exe")).is_file())
    })
}

pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, args: &[String], stdin: &[u8]) -> Result<CommandOutput, String> {
        let mut child = Command::new(PROGRAM)
            .args(args)
            .current_dir(std::env::temp_dir())
            // Claude Code refuses to start under this marker, and the plugin
            // skills run `heal semantic ask` from inside a session.
            .env_remove("CLAUDECODE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not start `{PROGRAM}`: {e}"))?;
        let mut input = child.stdin.take().expect("stdin is piped");
        let body = stdin.to_vec();
        // Threads, so a full pipe on any side cannot deadlock the others.
        let writer = std::thread::spawn(move || {
            let _ = input.write_all(&body);
        });
        let stdout = drain(child.stdout.take().expect("stdout is piped"));
        let stderr = drain(child.stderr.take().expect("stderr is piped"));
        let started = Instant::now();
        let timed_out = loop {
            if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                break false;
            }
            if started.elapsed() > REQUEST_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                break true;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let _ = writer.join();
        let stdout = stdout.join().unwrap_or_default();
        let stderr = stderr.join().unwrap_or_default();
        if timed_out {
            return Err(format!(
                "`{PROGRAM}` did not answer within {}s",
                REQUEST_TIMEOUT.as_secs()
            ));
        }
        Ok(CommandOutput { stdout, stderr })
    }
}

fn drain(mut pipe: impl Read + Send + 'static) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = pipe.read_to_string(&mut text);
        text
    })
}

/// The one result object of `claude -p --output-format json`.
#[derive(Debug, Deserialize)]
struct CliResult {
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    structured_output: Option<Value>,
    #[serde(default)]
    usage: CliUsage,
}

/// Claude Code caches its prompt, so `input_tokens` alone is only the
/// uncached remainder (measured: 2 on a real request, CLI 2.1.316).
#[allow(clippy::struct_field_names)] // mirrors the CLI's JSON keys
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CliUsage {
    input_tokens: u64,
    cache_creation_input_tokens: u64,
    cache_read_input_tokens: u64,
    output_tokens: u64,
}

impl From<&CliUsage> for Usage {
    fn from(u: &CliUsage) -> Self {
        Self {
            input_tokens: u.input_tokens
                + u.cache_creation_input_tokens
                + u.cache_read_input_tokens,
            output_tokens: u.output_tokens,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthStatus {
    #[serde(default)]
    logged_in: bool,
}

pub struct ClaudeCli {
    runner: Arc<dyn CommandRunner>,
    model: String,
    spend: SpendMeter,
    sleep: Sleeper,
}

impl ClaudeCli {
    #[must_use]
    pub fn new(runner: Arc<dyn CommandRunner>, model: String) -> Self {
        Self {
            runner,
            model,
            spend: SpendMeter::default(),
            sleep: Arc::new(std::thread::sleep),
        }
    }

    /// Replace the wait function. Tests pass a no-op so backoff costs nothing.
    #[must_use]
    pub fn with_sleeper(mut self, sleep: impl Fn(Duration) + Send + Sync + 'static) -> Self {
        self.sleep = Arc::new(sleep);
        self
    }

    fn ask_args(&self) -> Vec<String> {
        [
            "-p",
            "--model",
            &self.model,
            "--output-format",
            "json",
            "--json-schema",
            &answer_schema().to_string(),
            "--system-prompt",
            SYSTEM_PROMPT,
            "--effort",
            EFFORT,
            "--tools",
            "",
            "--setting-sources",
            "",
            "--strict-mcp-config",
            "--disable-slash-commands",
            "--no-session-persistence",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
    }

    fn ask_once(
        &self,
        state: &Value,
        questions: &BTreeMap<String, Question>,
    ) -> Result<Response, AskError> {
        let output = self
            .runner
            .run(&self.ask_args(), user_prompt(state, questions).as_bytes())
            .map_err(|e| error(AskErrorKind::Transient, e))?;
        let Ok(result) = serde_json::from_str::<CliResult>(output.stdout.trim()) else {
            let detail = if output.stderr.trim().is_empty() {
                &output.stdout
            } else {
                &output.stderr
            };
            return Err(error(classify_failure(detail), truncate(detail.trim())));
        };
        let usage = Usage::from(&result.usage);
        self.spend.record_call(&usage);
        if result.is_error {
            let message = result.result.unwrap_or_default();
            return Err(error(classify_failure(&message), truncate(&message)));
        }
        let raw = match (&result.structured_output, &result.result) {
            (Some(value), _) => RawAnswers::deserialize(value).map_err(|e| e.to_string()),
            (None, Some(text)) => serde_json::from_str(text).map_err(|e| e.to_string()),
            (None, None) => Err("no output".to_owned()),
        }
        .map_err(|e| {
            error(
                AskErrorKind::Other,
                format!("the answers are not the expected JSON: {e}"),
            )
        })?;
        Ok(Response {
            model: Some(self.model.clone()),
            answers: decode_answers(&raw, questions),
            usage,
        })
    }
}

fn error(kind: AskErrorKind, message: String) -> AskError {
    AskError {
        kind,
        status: 0,
        message,
    }
}

/// The CLI reports these failures as prose. Sign-in and account problems
/// fail every batch alike; a too-long prompt is the one a split can fix.
fn classify_failure(message: &str) -> AskErrorKind {
    let lower = message.to_lowercase();
    if lower.contains("prompt is too long") {
        AskErrorKind::TooBig
    } else if ["log in", "login", "api key", "credit balance", "hit your"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        AskErrorKind::Setup
    } else if ["rate limit", "overloaded", "timed out"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        AskErrorKind::Transient
    } else {
        AskErrorKind::Other
    }
}

impl Classifier for ClaudeCli {
    fn ask(
        &self,
        state: &Value,
        questions: &BTreeMap<String, Question>,
    ) -> Result<Response, AskError> {
        if questions.is_empty() {
            return Ok(Response::default());
        }
        let mut attempt = 0;
        loop {
            match self.ask_once(state, questions) {
                Err(e) if e.kind == AskErrorKind::Transient && attempt < RETRIES => {
                    attempt += 1;
                    self.spend.bump(|s| s.retried += 1);
                    (self.sleep)(backoff(attempt, None));
                }
                other => return other,
            }
        }
    }

    fn meter(&self) -> &SpendMeter {
        &self.spend
    }

    /// `claude auth status` reads the local sign-in; it cannot say whether
    /// the plan offers the model, so the first request settles that.
    fn check(&self) -> Result<ModelCheck, AskError> {
        let args = ["auth", "status", "--json"].map(str::to_owned);
        let output = self
            .runner
            .run(&args, b"")
            .map_err(|e| error(AskErrorKind::Setup, e))?;
        let status: AuthStatus = serde_json::from_str(output.stdout.trim()).map_err(|e| {
            error(
                AskErrorKind::Setup,
                format!("unreadable `claude auth status` output: {e}"),
            )
        })?;
        if !status.logged_in {
            return Err(error(
                AskErrorKind::Setup,
                "the claude CLI is not signed in; run `claude auth login`".to_owned(),
            ));
        }
        Ok(ModelCheck {
            model_available: None,
            note: format!(
                "claude CLI signed in; the first `heal semantic ask` request confirms model `{}`.",
                self.model
            ),
        })
    }

    fn max_concurrency(&self) -> usize {
        MAX_CONCURRENCY
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    /// Answers each run with the next scripted output, then repeats the last.
    struct Fake {
        outputs: Mutex<Vec<CommandOutput>>,
        seen: Mutex<Vec<(Vec<String>, String)>>,
    }

    impl Fake {
        fn new(stdout: &str, stderr: &str) -> Arc<Self> {
            Self::script(&[stdout], stderr)
        }

        fn script(stdouts: &[&str], stderr: &str) -> Arc<Self> {
            Arc::new(Self {
                outputs: Mutex::new(
                    stdouts
                        .iter()
                        .map(|s| CommandOutput {
                            stdout: (*s).to_owned(),
                            stderr: stderr.to_owned(),
                        })
                        .collect(),
                ),
                seen: Mutex::new(Vec::new()),
            })
        }
    }

    impl CommandRunner for Fake {
        fn run(&self, args: &[String], stdin: &[u8]) -> Result<CommandOutput, String> {
            self.seen
                .lock()
                .unwrap()
                .push((args.to_vec(), String::from_utf8_lossy(stdin).into_owned()));
            let mut outputs = self.outputs.lock().unwrap();
            Ok(if outputs.len() > 1 {
                outputs.remove(0)
            } else {
                outputs[0].clone()
            })
        }
    }

    fn one_noul() -> BTreeMap<String, Question> {
        BTreeMap::from([(
            "key".to_owned(),
            Question::Noul {
                instructions: json!("Is it pure?"),
                criteria: None,
            },
        )])
    }

    fn cli(fake: &Arc<Fake>) -> ClaudeCli {
        ClaudeCli::new(fake.clone(), "claude-haiku-5-5".to_owned()).with_sleeper(|_| {})
    }

    const ANSWER: &str =
        r#"{"answers":[{"id":"q0","probabilities":[{"option":"true","probability":0.9}]}]}"#;

    #[test]
    fn prompt_goes_over_stdin_and_the_child_gets_no_tools() {
        let stdout = json!({
            "type": "result",
            "is_error": false,
            "structured_output": serde_json::from_str::<Value>(ANSWER).unwrap(),
            "usage": {
                "input_tokens": 2,
                "cache_creation_input_tokens": 40,
                "cache_read_input_tokens": 8,
                "output_tokens": 10
            }
        })
        .to_string();
        let fake = Fake::new(&stdout, "");
        let c = cli(&fake);
        let r = c.ask(&json!("SECRET-STATE"), &one_noul()).unwrap();
        assert!((r.answers["key"].strength(0) - 0.9).abs() < 1e-9);
        assert_eq!(c.spend().input_tokens, 50);
        assert_eq!(c.spend().output_tokens, 10);

        let seen = fake.seen.lock().unwrap();
        let (args, stdin) = &seen[0];
        assert!(stdin.contains("SECRET-STATE"));
        assert!(args.iter().all(|a| !a.contains("SECRET-STATE")));
        let tools = args.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(args[tools + 1], "");
        let sources = args.iter().position(|a| a == "--setting-sources").unwrap();
        assert_eq!(args[sources + 1], "");
        assert!(args.iter().any(|a| a == "--strict-mcp-config"));
        assert!(args.iter().any(|a| a == "--no-session-persistence"));
    }

    #[test]
    fn answers_in_the_result_text_are_accepted() {
        let stdout = json!({"is_error": false, "result": ANSWER}).to_string();
        let c = cli(&Fake::new(&stdout, ""));
        assert_eq!(c.ask(&json!("s"), &one_noul()).unwrap().answers.len(), 1);
    }

    #[test]
    fn sign_in_failure_stops_the_run_and_other_failures_do_not() {
        let stdout = json!({"is_error": true, "result": "Not logged in · Please run /login"});
        let c = cli(&Fake::new(&stdout.to_string(), ""));
        let e = c.ask(&json!("s"), &one_noul()).unwrap_err();
        assert_eq!(e.kind, AskErrorKind::Setup);

        let stdout = json!({"is_error": true, "result": "Something else went wrong"});
        let c = cli(&Fake::new(&stdout.to_string(), ""));
        let e = c.ask(&json!("s"), &one_noul()).unwrap_err();
        assert_eq!(e.kind, AskErrorKind::Other);
    }

    #[test]
    fn transient_failures_are_retried_and_then_given_up() {
        let overloaded = json!({"is_error": true, "result": "API Error: overloaded"}).to_string();
        let answered = json!({"is_error": false, "result": ANSWER}).to_string();
        let fake = Fake::script(&[&overloaded, &answered], "");
        let c = cli(&fake);
        assert_eq!(c.ask(&json!("s"), &one_noul()).unwrap().answers.len(), 1);
        assert_eq!(c.spend().retried, 1);
        assert_eq!(fake.seen.lock().unwrap().len(), 2);

        let fake = Fake::new(&overloaded, "");
        let c = cli(&fake);
        let e = c.ask(&json!("s"), &one_noul()).unwrap_err();
        assert_eq!(e.kind, AskErrorKind::Transient);
        assert_eq!(fake.seen.lock().unwrap().len(), 1 + RETRIES as usize);
    }

    #[test]
    fn other_failures_are_not_retried() {
        let stdout = json!({"is_error": true, "result": "Something else went wrong"});
        let fake = Fake::new(&stdout.to_string(), "");
        cli(&fake).ask(&json!("s"), &one_noul()).unwrap_err();
        assert_eq!(fake.seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn non_json_output_reports_stderr() {
        let c = cli(&Fake::new("", "error: unknown option"));
        let e = c.ask(&json!("s"), &one_noul()).unwrap_err();
        assert!(e.message.contains("unknown option"), "{}", e.message);
    }

    #[test]
    fn too_long_prompt_is_recoverable_by_splitting() {
        assert_eq!(classify_failure("Prompt is too long"), AskErrorKind::TooBig);
        assert_eq!(
            classify_failure("API Error: overloaded"),
            AskErrorKind::Transient
        );
        assert_eq!(
            classify_failure("You've hit your limit · resets 3pm"),
            AskErrorKind::Setup
        );
    }

    #[test]
    fn check_reads_the_local_sign_in() {
        let c = cli(&Fake::new(
            r#"{"loggedIn":true,"authMethod":"claude.ai"}"#,
            "",
        ));
        assert_eq!(c.check().unwrap().model_available, None);
        let c = cli(&Fake::new(r#"{"loggedIn":false}"#, ""));
        assert_eq!(c.check().unwrap_err().kind, AskErrorKind::Setup);
    }
}
