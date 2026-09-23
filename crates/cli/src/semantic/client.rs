//! Jev HTTP client.
//!
//! This module and `commands::auth` are the only places HEAL opens a
//! network connection. Everything is blocking: a run is a bounded fan-out
//! of independent requests, and the scoped threads in
//! [`crate::semantic::runner`] are enough to keep the server's token
//! bucket busy without an async runtime.
//!
//! Retry policy, split-on-`max_tokens_exceeded`, and the failure
//! taxonomy follow mizchi/jev-lint's measured client (`src/jev.ts`, MIT).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::semantic::api::{Question, Request, Response};
use crate::semantic::cost::{estimate_tokens, usd_for};
use crate::semantic::pacer::Pacer;

pub const DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";

/// Environment variables consulted for the endpoint, in order. Mirrors the
/// key lookup so a proxy or a test server can be pointed at explicitly.
pub const BASE_URL_VARS: [&str; 2] = ["TYPESAFE_BASE_URL", "TYPESAFEAI_BASE_URL"];

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_RETRIES: u32 = 4;
const DEFAULT_RATE_LIMIT_RETRIES: u32 = 8;

/// One HTTP exchange, stripped to what the client needs.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    pub retry_after: Option<String>,
}

/// The network boundary. Production uses [`UreqTransport`]; tests hand in
/// a fake so no test ever opens a socket.
pub trait Transport: Send + Sync {
    fn post_json(&self, url: &str, api_key: &str, body: &[u8]) -> Result<HttpResponse, String>;
    fn get(&self, url: &str, api_key: &str) -> Result<HttpResponse, String>;
}

/// rustls with the OS trust store (rustls-native-certs), so a corporate
/// TLS-inspecting proxy whose CA is installed on the machine keeps working.
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    pub fn new() -> Result<Self, String> {
        let loaded = rustls_native_certs::load_native_certs();
        if loaded.certs.is_empty() {
            return Err(format!(
                "no trusted root certificates found in the OS store ({} error(s)); \
                 install the system CA bundle or point SSL_CERT_FILE at one",
                loaded.errors.len()
            ));
        }
        let certs: Vec<ureq::tls::Certificate<'static>> = loaded
            .certs
            .iter()
            .map(|der| ureq::tls::Certificate::from_der(der.as_ref()).to_owned())
            .collect();
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::Rustls)
            .root_certs(ureq::tls::RootCerts::new_with_certs(&certs))
            .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .build();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .http_status_as_error(false)
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build()
            .into();
        Ok(Self { agent })
    }

    fn finish(
        result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<HttpResponse, String> {
        let mut res = result.map_err(|e| e.to_string())?;
        let status = res.status().as_u16();
        let retry_after = res
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = res.body_mut().read_to_string().map_err(|e| e.to_string())?;
        Ok(HttpResponse {
            status,
            body,
            retry_after,
        })
    }
}

impl Transport for UreqTransport {
    fn post_json(&self, url: &str, api_key: &str, body: &[u8]) -> Result<HttpResponse, String> {
        Self::finish(
            self.agent
                .post(url)
                .header("authorization", &format!("Bearer {api_key}"))
                .header("content-type", "application/json")
                .send(body),
        )
    }

    fn get(&self, url: &str, api_key: &str) -> Result<HttpResponse, String> {
        Self::finish(
            self.agent
                .get(url)
                .header("authorization", &format!("Bearer {api_key}"))
                .call(),
        )
    }
}

/// What the caller can do about a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JevErrorKind {
    /// Send fewer questions (the one recoverable 400).
    TooBig,
    /// Fix the key or the account; retrying will not help.
    Auth,
    /// Retried already; the network or the service failed.
    Transient,
    /// Give up on this batch.
    Other,
}

#[derive(Debug, Clone)]
pub struct JevError {
    pub kind: JevErrorKind,
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for JevError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.status == 0 {
            write!(f, "{}", self.message)
        } else {
            write!(f, "HTTP {}: {}", self.status, self.message)
        }
    }
}

impl std::error::Error for JevError {}

impl JevError {
    fn classify(status: u16, body: &str) -> JevErrorKind {
        match status {
            400 if body.contains("max_tokens_exceeded") => JevErrorKind::TooBig,
            401..=403 => JevErrorKind::Auth,
            500..=599 => JevErrorKind::Transient,
            _ => JevErrorKind::Other,
        }
    }
}

/// Running totals for the report and the `max_usd` guard.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Spend {
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub retried: u64,
    pub rate_limited: u64,
    pub splits: u64,
}

impl Spend {
    #[must_use]
    pub fn usd(&self) -> f64 {
        usd_for(self.input_tokens)
    }
}

type Sleeper = Arc<dyn Fn(Duration) + Send + Sync>;

pub struct JevClient {
    transport: Arc<dyn Transport>,
    api_key: String,
    base_url: String,
    model: String,
    retries: u32,
    rate_limit_retries: u32,
    pacer: Mutex<Pacer>,
    spend: Mutex<Spend>,
    served_model: Mutex<Option<String>>,
    sleep: Sleeper,
}

impl JevClient {
    #[must_use]
    pub fn new(transport: Arc<dyn Transport>, api_key: String, model: String) -> Self {
        let base_url = BASE_URL_VARS
            .iter()
            .find_map(|v| std::env::var(v).ok().filter(|s| !s.trim().is_empty()))
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        Self {
            transport,
            api_key,
            base_url: base_url.trim().trim_end_matches('/').to_owned(),
            model,
            retries: DEFAULT_RETRIES,
            rate_limit_retries: DEFAULT_RATE_LIMIT_RETRIES,
            pacer: Mutex::new(Pacer::default()),
            spend: Mutex::new(Spend::default()),
            served_model: Mutex::new(None),
            sleep: Arc::new(std::thread::sleep),
        }
    }

    /// Replace the wait function. Tests pass a no-op so backoff costs nothing.
    #[must_use]
    pub fn with_sleeper(mut self, sleep: impl Fn(Duration) + Send + Sync + 'static) -> Self {
        self.sleep = Arc::new(sleep);
        self
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: &str) -> Self {
        base_url
            .trim_end_matches('/')
            .clone_into(&mut self.base_url);
        self
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn spend(&self) -> Spend {
        *self.spend.lock().expect("spend lock")
    }

    pub fn served_model(&self) -> Option<String> {
        self.served_model.lock().expect("model lock").clone()
    }

    /// `GET /v1/models`: the model ids the key can use. Backs
    /// `heal auth jev status` and `heal semantic ask --check`.
    pub fn list_models(&self) -> Result<Vec<String>, JevError> {
        let url = format!("{}/v1/models", self.base_url);
        let res = self
            .transport
            .get(&url, &self.api_key)
            .map_err(|e| JevError {
                kind: JevErrorKind::Transient,
                status: 0,
                message: format!("network: {e}"),
            })?;
        if !(200..300).contains(&res.status) {
            return Err(JevError {
                kind: JevError::classify(res.status, &res.body),
                status: res.status,
                message: truncate(&res.body),
            });
        }
        let v: Value = serde_json::from_str(&res.body).map_err(|e| JevError {
            kind: JevErrorKind::Other,
            status: res.status,
            message: format!("unparseable models response: {e}"),
        })?;
        Ok(model_ids(&v))
    }

    /// One request: one state, N questions, N answers.
    pub fn ask(
        &self,
        state: &Value,
        questions: &BTreeMap<String, Question>,
    ) -> Result<Response, JevError> {
        if questions.is_empty() {
            return Ok(Response::default());
        }
        let body = serde_json::to_vec(&Request {
            model: &self.model,
            state,
            questions,
        })
        .expect("request serialization is infallible");
        #[allow(clippy::cast_precision_loss)]
        let estimated = estimate_tokens(body.len()) as f64;
        let url = format!("{}/v1/systemone", self.base_url);

        let mut attempt = 0;
        let mut limited = 0;
        loop {
            self.wait_for_budget(estimated);
            let result = self.transport.post_json(&url, &self.api_key, &body);
            let res = match result {
                Ok(res) => res,
                Err(e) => {
                    if attempt == self.retries {
                        return Err(JevError {
                            kind: JevErrorKind::Transient,
                            status: 0,
                            message: format!("network: {e}"),
                        });
                    }
                    attempt += 1;
                    self.bump(|s| s.retried += 1);
                    (self.sleep)(backoff(attempt, None));
                    continue;
                }
            };
            if (200..300).contains(&res.status) {
                let parsed: Response = serde_json::from_str(&res.body).map_err(|e| JevError {
                    kind: JevErrorKind::Other,
                    status: res.status,
                    message: format!("unparseable response: {e}"),
                })?;
                #[allow(clippy::cast_precision_loss)]
                let actual = parsed.usage.input_tokens as f64;
                self.pacer
                    .lock()
                    .expect("pacer lock")
                    .settle(estimated, actual);
                self.bump(|s| {
                    s.calls += 1;
                    s.input_tokens += parsed.usage.input_tokens;
                    s.output_tokens += parsed.usage.output_tokens;
                });
                if let Some(m) = &parsed.model {
                    *self.served_model.lock().expect("model lock") = Some(m.clone());
                }
                return Ok(parsed);
            }
            if res.status == 429 {
                self.pacer
                    .lock()
                    .expect("pacer lock")
                    .throttled(Instant::now());
                self.bump(|s| s.rate_limited += 1);
                if limited == self.rate_limit_retries {
                    return Err(JevError {
                        kind: JevErrorKind::Transient,
                        status: 429,
                        message: "rate limited".to_owned(),
                    });
                }
                limited += 1;
                (self.sleep)(rate_limit_wait(limited, res.retry_after.as_deref()));
                continue;
            }
            // 529 is the documented "service overloaded" status.
            let kind = if res.status == 529 {
                JevErrorKind::Transient
            } else {
                JevError::classify(res.status, &res.body)
            };
            if kind != JevErrorKind::Transient || attempt == self.retries {
                return Err(JevError {
                    kind,
                    status: res.status,
                    message: truncate(&res.body),
                });
            }
            attempt += 1;
            self.bump(|s| s.retried += 1);
            (self.sleep)(backoff(attempt, res.retry_after.as_deref()));
        }
    }

    /// Ask, halving the question set whenever the server reports the
    /// request too big. The state is unchanged by a split, so a state that
    /// alone exceeds the ceiling cannot be rescued here — the planner must
    /// have kept it under [`crate::semantic::cost::state_budget`].
    pub fn ask_splitting(
        &self,
        state: &Value,
        questions: &BTreeMap<String, Question>,
    ) -> Result<Response, JevError> {
        match self.ask(state, questions) {
            Err(e) if e.kind == JevErrorKind::TooBig && questions.len() >= 2 => {
                self.bump(|s| s.splits += 1);
                let half = questions.len().div_ceil(2);
                let mut merged = Response::default();
                let (a, b): (Vec<_>, Vec<_>) =
                    questions.iter().enumerate().partition(|(i, _)| *i < half);
                for part in [a, b] {
                    let subset: BTreeMap<String, Question> = part
                        .into_iter()
                        .map(|(_, (k, q))| (k.clone(), q.clone()))
                        .collect();
                    let res = self.ask_splitting(state, &subset)?;
                    merged.answers.extend(res.answers);
                    merged.usage.input_tokens += res.usage.input_tokens;
                    merged.usage.output_tokens += res.usage.output_tokens;
                    if merged.model.is_none() {
                        merged.model = res.model;
                    }
                }
                Ok(merged)
            }
            other => other,
        }
    }

    fn wait_for_budget(&self, tokens: f64) {
        loop {
            let wait = {
                let mut p = self.pacer.lock().expect("pacer lock");
                let wait = p.delay(tokens, Instant::now());
                if wait.is_zero() {
                    p.charge(tokens);
                }
                wait
            };
            if wait.is_zero() {
                return;
            }
            (self.sleep)(wait);
        }
    }

    fn bump(&self, f: impl FnOnce(&mut Spend)) {
        f(&mut self.spend.lock().expect("spend lock"));
    }
}

/// Accepts both `{"data":[{"id":..}]}` (OpenAI-style) and a bare list of
/// ids or objects, since the public reference documents the endpoint but
/// not the envelope.
fn model_ids(v: &Value) -> Vec<String> {
    let list = v
        .get("data")
        .or_else(|| v.get("models"))
        .unwrap_or(v)
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut ids: Vec<String> = list
        .iter()
        .filter_map(|item| {
            item.as_str()
                .map(str::to_owned)
                .or_else(|| item.get("id").and_then(Value::as_str).map(str::to_owned))
        })
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

fn truncate(body: &str) -> String {
    body.chars().take(240).collect()
}

fn hinted(retry_after: Option<&str>) -> Option<Duration> {
    retry_after
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(Duration::from_secs_f64)
}

fn backoff(attempt: u32, retry_after: Option<&str>) -> Duration {
    hinted(retry_after).unwrap_or_else(|| {
        let ms = (500_u64 << attempt.min(6)).min(20_000);
        Duration::from_millis(ms)
    })
}

fn rate_limit_wait(nth: u32, retry_after: Option<&str>) -> Duration {
    hinted(retry_after).unwrap_or_else(|| {
        let ms = 300.0 * 1.5_f64.powi(i32::try_from(nth.saturating_sub(1)).unwrap_or(i32::MAX));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Duration::from_millis(ms.min(5_000.0) as u64)
    })
}

#[cfg(test)]
pub(crate) mod testing {
    //! Scripted transport shared by semantic tests.
    use super::{HttpResponse, Transport};
    use std::sync::Mutex;

    type Handler = Box<dyn Fn(&str, &[u8]) -> Result<HttpResponse, String> + Send + Sync>;

    pub struct FakeTransport {
        handler: Handler,
        pub requests: Mutex<Vec<String>>,
    }

    impl FakeTransport {
        pub fn new(
            handler: impl Fn(&str, &[u8]) -> Result<HttpResponse, String> + Send + Sync + 'static,
        ) -> Self {
            Self {
                handler: Box::new(handler),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl Transport for FakeTransport {
        fn post_json(&self, url: &str, _key: &str, body: &[u8]) -> Result<HttpResponse, String> {
            self.requests
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(body).into_owned());
            (self.handler)(url, body)
        }
        fn get(&self, url: &str, _key: &str) -> Result<HttpResponse, String> {
            (self.handler)(url, b"")
        }
    }

    pub fn ok(body: &serde_json::Value) -> HttpResponse {
        HttpResponse {
            status: 200,
            body: body.to_string(),
            retry_after: None,
        }
    }

    /// Answers every `noul` question in the request with `p`.
    pub fn answer_all_noul(body: &[u8], p: f64) -> HttpResponse {
        let req: serde_json::Value = serde_json::from_slice(body).unwrap();
        let mut answers = serde_json::Map::new();
        for key in req["questions"].as_object().unwrap().keys() {
            answers.insert(key.clone(), serde_json::json!({"type": "noul", "noul": p}));
        }
        ok(&serde_json::json!({
            "model": "jev-1.13.0",
            "answers": answers,
            "usage": {"input_tokens": body.len() / 4, "output_tokens": 1}
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{answer_all_noul, ok, FakeTransport};
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn noul(text: &str) -> Question {
        Question::Noul {
            instructions: json!(text),
            criteria: None,
        }
    }

    fn client(t: FakeTransport) -> JevClient {
        JevClient::new(Arc::new(t), "k".into(), "jev-1.13.0".into())
            .with_base_url("http://test")
            .with_sleeper(|_| {})
    }

    #[test]
    fn ask_returns_answers_and_counts_spend() {
        let c = client(FakeTransport::new(|_, body| Ok(answer_all_noul(body, 0.9))));
        let mut qs = BTreeMap::new();
        qs.insert("a".to_owned(), noul("x"));
        let r = c.ask(&json!("state"), &qs).unwrap();
        assert!(r.answers.contains_key("a"));
        assert_eq!(c.spend().calls, 1);
        assert!(c.spend().input_tokens > 0);
        assert_eq!(c.served_model().as_deref(), Some("jev-1.13.0"));
    }

    #[test]
    fn auth_failure_is_not_retried() {
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let c = client(FakeTransport::new(move |_, _| {
            n2.fetch_add(1, Ordering::SeqCst);
            Ok(HttpResponse {
                status: 401,
                body: "bad key".into(),
                retry_after: None,
            })
        }));
        let mut qs = BTreeMap::new();
        qs.insert("a".to_owned(), noul("x"));
        let e = c.ask(&json!("s"), &qs).unwrap_err();
        assert_eq!(e.kind, JevErrorKind::Auth);
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn server_errors_and_rate_limits_are_retried() {
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let c = client(FakeTransport::new(move |_, body| {
            let i = n2.fetch_add(1, Ordering::SeqCst);
            Ok(match i {
                0 => HttpResponse {
                    status: 503,
                    body: String::new(),
                    retry_after: None,
                },
                1 => HttpResponse {
                    status: 429,
                    body: String::new(),
                    retry_after: None,
                },
                _ => answer_all_noul(body, 0.2),
            })
        }));
        let mut qs = BTreeMap::new();
        qs.insert("a".to_owned(), noul("x"));
        assert!(c.ask(&json!("s"), &qs).is_ok());
        let s = c.spend();
        assert_eq!((s.retried, s.rate_limited, s.calls), (1, 1, 1));
    }

    #[test]
    fn too_big_splits_the_question_set_until_it_fits() {
        let c = client(FakeTransport::new(|_, body| {
            let req: Value = serde_json::from_slice(body).unwrap();
            if req["questions"].as_object().unwrap().len() > 2 {
                return Ok(HttpResponse {
                    status: 400,
                    body: r#"{"error":"max_tokens_exceeded"}"#.into(),
                    retry_after: None,
                });
            }
            Ok(answer_all_noul(body, 0.7))
        }));
        let qs: BTreeMap<String, Question> = (0..7).map(|i| (format!("q{i}"), noul("x"))).collect();
        let r = c.ask_splitting(&json!("s"), &qs).unwrap();
        assert_eq!(r.answers.len(), 7);
        assert!(c.spend().splits >= 3);
    }

    #[test]
    fn list_models_accepts_common_envelopes() {
        let c = client(FakeTransport::new(|_, _| {
            Ok(ok(
                &json!({"data": [{"id": "jev-1.13.0"}, {"id": "jev-latest"}]}),
            ))
        }));
        assert_eq!(c.list_models().unwrap(), vec!["jev-1.13.0", "jev-latest"]);
        assert_eq!(model_ids(&json!(["b", "a"])), vec!["a", "b"]);
    }

    #[test]
    fn backoff_honours_retry_after_and_caps() {
        assert_eq!(backoff(1, Some("2")), Duration::from_secs(2));
        assert!(backoff(10, None) <= Duration::from_secs(20));
        assert!(rate_limit_wait(30, None) <= Duration::from_secs(5));
    }
}
