//! HTTP client for the semantic providers.
//!
//! This module and `commands::auth` are the only places HEAL opens a
//! network connection. Everything is blocking: a run is a bounded fan-out
//! of independent requests, and the scoped threads in
//! [`crate::semantic::runner`] are enough to keep the server's token
//! bucket busy without an async runtime. Retries, pacing, and spend are
//! shared; a [`Dialect`] holds what differs between providers.
//!
//! Retry policy, split-on-`max_tokens_exceeded`, and the failure
//! taxonomy follow mizchi/jev-lint's measured client (`src/jev.ts`, MIT).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::semantic::api::{Question, Request, Response, Usage};
use crate::semantic::claude::ClaudeDialect;
use crate::semantic::cost::estimate_tokens;
use crate::semantic::pacer::Pacer;

/// The Jev endpoint (`scope.md` R5). Deliberately not configurable: an
/// override would send project content and the key to wherever it pointed.
pub const JEV_BASE_URL: &str = "https://api.typesafe.ai";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_RETRIES: u32 = 4;
const DEFAULT_RATE_LIMIT_RETRIES: u32 = 8;

/// Request headers, including the provider's authentication.
pub type Headers = [(&'static str, String)];

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
    fn post_json(&self, url: &str, headers: &Headers, body: &[u8]) -> Result<HttpResponse, String>;
    fn get(&self, url: &str, headers: &Headers) -> Result<HttpResponse, String>;
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
            // A followed 307/308 would re-send the request body (project
            // content) to another host; surface the redirect as an error.
            .max_redirects(0)
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
    fn post_json(&self, url: &str, headers: &Headers, body: &[u8]) -> Result<HttpResponse, String> {
        let mut request = self
            .agent
            .post(url)
            .header("content-type", "application/json");
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        Self::finish(request.send(body))
    }

    fn get(&self, url: &str, headers: &Headers) -> Result<HttpResponse, String> {
        let mut request = self.agent.get(url);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        Self::finish(request.call())
    }
}

/// What the caller can do about a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskErrorKind {
    /// Send fewer questions (the one recoverable failure).
    TooBig,
    /// Fix the key, the account, or `[features.semantic].model`;
    /// retrying will not help, and neither will any other batch.
    Setup,
    /// Retried already; the network or the service failed.
    Transient,
    /// Give up on this batch.
    Other,
}

#[derive(Debug, Clone)]
pub struct AskError {
    pub kind: AskErrorKind,
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for AskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.status == 0 {
            write!(f, "{}", self.message)
        } else {
            write!(f, "HTTP {}: {}", self.status, self.message)
        }
    }
}

impl std::error::Error for AskError {}

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

/// The shared [`Spend`] of one client, updated from parallel requests.
#[derive(Debug, Default)]
pub struct SpendMeter(Mutex<Spend>);

impl SpendMeter {
    pub fn bump(&self, f: impl FnOnce(&mut Spend)) {
        f(&mut self.0.lock().expect("spend lock"));
    }

    pub fn record_call(&self, usage: &Usage) {
        self.bump(|s| {
            s.calls += 1;
            s.input_tokens += usage.input_tokens;
            s.output_tokens += usage.output_tokens;
        });
    }

    pub fn get(&self) -> Spend {
        *self.0.lock().expect("spend lock")
    }
}

/// Outcome of a credentials check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCheck {
    /// `None` when the check cannot tell whether the model is usable.
    pub model_available: Option<bool>,
    /// One sentence for the terminal.
    pub note: String,
}

/// Something that answers typed questions about a state. The runner only
/// sees this, so a task behaves the same whichever backend answers.
pub trait Classifier: Send + Sync {
    /// One request: one state, N questions, N answers.
    fn ask(
        &self,
        state: &Value,
        questions: &BTreeMap<String, Question>,
    ) -> Result<Response, AskError>;

    fn meter(&self) -> &SpendMeter;

    fn record_split(&self) {
        self.meter().bump(|s| s.splits += 1);
    }

    fn spend(&self) -> Spend {
        self.meter().get()
    }

    /// Confirm the credentials without asking anything.
    fn check(&self) -> Result<ModelCheck, AskError>;

    /// Upper bound on requests in flight, on top of
    /// `[features.semantic].concurrency`.
    fn max_concurrency(&self) -> usize {
        usize::MAX
    }

    /// Ask, halving the question set whenever the answer reports the
    /// request too big. The state is unchanged by a split, so a state that
    /// alone exceeds the ceiling cannot be rescued here — the planner must
    /// have kept it under [`crate::semantic::cost::state_budget`].
    fn ask_splitting(
        &self,
        state: &Value,
        questions: &BTreeMap<String, Question>,
    ) -> Result<Response, AskError> {
        match self.ask(state, questions) {
            Err(e) if e.kind == AskErrorKind::TooBig && questions.len() >= 2 => {
                self.record_split();
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
}

/// A 2xx body after decoding. The tokens were paid for even when the
/// response cannot be used, so `failure` travels next to the usage.
#[derive(Debug, Clone, Default)]
pub struct Decoded {
    pub response: Response,
    pub failure: Option<(AskErrorKind, String)>,
}

/// What differs between two HTTP providers.
pub trait Dialect: Send + Sync {
    fn base_url(&self) -> &'static str;
    fn ask_path(&self) -> &'static str;
    fn models_path(&self) -> &'static str;
    fn auth_headers(&self, api_key: &str) -> Vec<(&'static str, String)>;
    fn request_body(
        &self,
        model: &str,
        state: &Value,
        questions: &BTreeMap<String, Question>,
    ) -> Vec<u8>;
    /// Reads a 2xx body. `Err` means the body is not this provider's
    /// response shape at all.
    fn decode(&self, body: &str, questions: &BTreeMap<String, Question>)
        -> Result<Decoded, String>;
    fn classify(&self, status: u16, body: &str) -> AskErrorKind;
    fn model_check(&self, model: &str, listed: &[String]) -> ModelCheck;
}

/// `TypeSafe`'s System One endpoint.
pub struct JevDialect;

impl Dialect for JevDialect {
    fn base_url(&self) -> &'static str {
        JEV_BASE_URL
    }

    fn ask_path(&self) -> &'static str {
        "/v1/systemone"
    }

    fn models_path(&self) -> &'static str {
        "/v1/models"
    }

    fn auth_headers(&self, api_key: &str) -> Vec<(&'static str, String)> {
        vec![("authorization", format!("Bearer {api_key}"))]
    }

    fn request_body(
        &self,
        model: &str,
        state: &Value,
        questions: &BTreeMap<String, Question>,
    ) -> Vec<u8> {
        serde_json::to_vec(&Request {
            model,
            state,
            questions,
        })
        .expect("request serialization is infallible")
    }

    fn decode(
        &self,
        body: &str,
        _questions: &BTreeMap<String, Question>,
    ) -> Result<Decoded, String> {
        let response: Response = serde_json::from_str(body).map_err(|e| e.to_string())?;
        Ok(Decoded {
            response,
            failure: None,
        })
    }

    /// An id the server does not know comes back as HTTP 400
    /// `{"detail":{"error_type":"api_usage_error","message":"Unknown
    /// model: <id>"}}` (measured against the live API, 2026-09-23).
    fn classify(&self, status: u16, body: &str) -> AskErrorKind {
        match status {
            400 if body.contains("max_tokens_exceeded") => AskErrorKind::TooBig,
            400 if body.contains("Unknown model") => AskErrorKind::Setup,
            401..=403 => AskErrorKind::Setup,
            500..=599 => AskErrorKind::Transient,
            _ => AskErrorKind::Other,
        }
    }

    /// Unlisted is not an error here: the listing names only the moving
    /// aliases, so a pinned version is confirmed by the first
    /// `heal semantic ask` request instead.
    fn model_check(&self, model: &str, listed: &[String]) -> ModelCheck {
        if model_listed(listed, model) {
            ModelCheck {
                model_available: Some(true),
                note: format!("Jev API reachable; model `{model}` available."),
            }
        } else {
            ModelCheck {
                model_available: None,
                note: format!(
                    "Jev API reachable; key accepted. Model `{model}` is not in the API's model \
                     list ({}); pinned versions are not listed, and an unknown one stops the \
                     first `heal semantic ask` request.",
                    if listed.is_empty() {
                        "empty".to_owned()
                    } else {
                        listed.join(", ")
                    }
                ),
            }
        }
    }
}

pub(crate) type Sleeper = Arc<dyn Fn(Duration) + Send + Sync>;

pub struct ApiClient {
    transport: Arc<dyn Transport>,
    dialect: Arc<dyn Dialect>,
    headers: Vec<(&'static str, String)>,
    base_url: String,
    model: String,
    retries: u32,
    rate_limit_retries: u32,
    pacer: Mutex<Pacer>,
    spend: SpendMeter,
    served_model: Mutex<Option<String>>,
    sleep: Sleeper,
}

impl ApiClient {
    #[must_use]
    pub fn new(
        transport: Arc<dyn Transport>,
        dialect: Arc<dyn Dialect>,
        api_key: &str,
        model: String,
    ) -> Self {
        Self {
            transport,
            headers: dialect.auth_headers(api_key),
            base_url: dialect.base_url().to_owned(),
            dialect,
            model,
            retries: DEFAULT_RETRIES,
            rate_limit_retries: DEFAULT_RATE_LIMIT_RETRIES,
            pacer: Mutex::new(Pacer::default()),
            spend: SpendMeter::default(),
            served_model: Mutex::new(None),
            sleep: Arc::new(std::thread::sleep),
        }
    }

    #[must_use]
    pub fn jev(transport: Arc<dyn Transport>, api_key: &str, model: String) -> Self {
        Self::new(transport, Arc::new(JevDialect), api_key, model)
    }

    #[must_use]
    pub fn claude(transport: Arc<dyn Transport>, api_key: &str, model: String) -> Self {
        Self::new(transport, Arc::new(ClaudeDialect), api_key, model)
    }

    /// Replace the wait function. Tests pass a no-op so backoff costs nothing.
    #[must_use]
    pub fn with_sleeper(mut self, sleep: impl Fn(Duration) + Send + Sync + 'static) -> Self {
        self.sleep = Arc::new(sleep);
        self
    }

    /// Test-only: point a client with a fake transport at a dummy host.
    #[cfg(test)]
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

    pub fn served_model(&self) -> Option<String> {
        self.served_model.lock().expect("model lock").clone()
    }

    /// `GET /v1/models`: the model ids the key can use. Backs
    /// `heal auth <provider> status` and `heal semantic ask --check`.
    pub fn list_models(&self) -> Result<Vec<String>, AskError> {
        let url = format!("{}{}", self.base_url, self.dialect.models_path());
        let res = self
            .transport
            .get(&url, &self.headers)
            .map_err(|e| AskError {
                kind: AskErrorKind::Transient,
                status: 0,
                message: format!("network: {e}"),
            })?;
        if !(200..300).contains(&res.status) {
            return Err(AskError {
                kind: self.dialect.classify(res.status, &res.body),
                status: res.status,
                message: truncate(&res.body),
            });
        }
        let v: Value = serde_json::from_str(&res.body).map_err(|e| AskError {
            kind: AskErrorKind::Other,
            status: res.status,
            message: format!("unparseable models response: {e}"),
        })?;
        Ok(model_ids(&v))
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
}

impl Classifier for ApiClient {
    fn ask(
        &self,
        state: &Value,
        questions: &BTreeMap<String, Question>,
    ) -> Result<Response, AskError> {
        if questions.is_empty() {
            return Ok(Response::default());
        }
        let body = self.dialect.request_body(&self.model, state, questions);
        #[allow(clippy::cast_precision_loss)]
        let estimated = estimate_tokens(body.len()) as f64;
        let url = format!("{}{}", self.base_url, self.dialect.ask_path());

        let mut attempt = 0;
        let mut limited = 0;
        loop {
            self.wait_for_budget(estimated);
            let result = self.transport.post_json(&url, &self.headers, &body);
            let res = match result {
                Ok(res) => res,
                Err(e) => {
                    if attempt == self.retries {
                        return Err(AskError {
                            kind: AskErrorKind::Transient,
                            status: 0,
                            message: format!("network: {e}"),
                        });
                    }
                    attempt += 1;
                    self.spend.bump(|s| s.retried += 1);
                    (self.sleep)(backoff(attempt, None));
                    continue;
                }
            };
            if (200..300).contains(&res.status) {
                let decoded = self
                    .dialect
                    .decode(&res.body, questions)
                    .map_err(|e| AskError {
                        kind: AskErrorKind::Other,
                        status: res.status,
                        message: format!("unparseable response: {e}"),
                    })?;
                let parsed = decoded.response;
                #[allow(clippy::cast_precision_loss)]
                let actual = parsed.usage.input_tokens as f64;
                self.pacer
                    .lock()
                    .expect("pacer lock")
                    .settle(estimated, actual);
                self.spend.record_call(&parsed.usage);
                if let Some(m) = &parsed.model {
                    *self.served_model.lock().expect("model lock") = Some(m.clone());
                }
                if let Some((kind, message)) = decoded.failure {
                    return Err(AskError {
                        kind,
                        status: res.status,
                        message,
                    });
                }
                return Ok(parsed);
            }
            if res.status == 429 {
                self.pacer
                    .lock()
                    .expect("pacer lock")
                    .throttled(Instant::now());
                self.spend.bump(|s| s.rate_limited += 1);
                if limited == self.rate_limit_retries {
                    return Err(AskError {
                        kind: AskErrorKind::Transient,
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
                AskErrorKind::Transient
            } else {
                self.dialect.classify(res.status, &res.body)
            };
            if kind != AskErrorKind::Transient || attempt == self.retries {
                return Err(AskError {
                    kind,
                    status: res.status,
                    message: truncate(&res.body),
                });
            }
            attempt += 1;
            self.spend.bump(|s| s.retried += 1);
            (self.sleep)(backoff(attempt, res.retry_after.as_deref()));
        }
    }

    fn meter(&self) -> &SpendMeter {
        &self.spend
    }

    fn check(&self) -> Result<ModelCheck, AskError> {
        let listed = self.list_models()?;
        Ok(self.dialect.model_check(&self.model, &listed))
    }
}

/// Whether `model` appears in a `GET /v1/models` listing. For Jev, not
/// being listed does not make a model unusable: the listing names only the
/// moving aliases (`jev-latest`, `jev-preview`), while pinned versions
/// such as `jev-1.13.0` are accepted by `/v1/systemone` without being
/// listed (measured against the live API, 2026-09-23). An id the server
/// does not know is caught by the first `/v1/systemone` request instead,
/// as an [`AskErrorKind::Setup`] error that stops the run.
#[must_use]
pub fn model_listed(models: &[String], model: &str) -> bool {
    models.iter().any(|m| m == model)
}

/// The live API answers `{"models":[{"name","description",
/// "release_date"}]}` (measured, 2026-09-23). An OpenAI-style
/// `{"data":[{"id":..}]}` and a bare list of ids are accepted too, since
/// the public reference documents the endpoint but not the envelope.
fn model_ids(v: &Value) -> Vec<String> {
    let list = v
        .get("models")
        .or_else(|| v.get("data"))
        .unwrap_or(v)
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut ids: Vec<String> = list
        .iter()
        .filter_map(|item| {
            item.as_str().map(str::to_owned).or_else(|| {
                ["name", "id"]
                    .iter()
                    .find_map(|k| item.get(*k).and_then(Value::as_str))
                    .map(str::to_owned)
            })
        })
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

pub(crate) fn truncate(body: &str) -> String {
    body.chars().take(240).collect()
}

fn hinted(retry_after: Option<&str>) -> Option<Duration> {
    retry_after
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(Duration::from_secs_f64)
}

pub(crate) fn backoff(attempt: u32, retry_after: Option<&str>) -> Duration {
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
    use super::{Headers, HttpResponse, Transport};
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
        fn post_json(
            &self,
            url: &str,
            _headers: &Headers,
            body: &[u8],
        ) -> Result<HttpResponse, String> {
            self.requests
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(body).into_owned());
            (self.handler)(url, body)
        }
        fn get(&self, url: &str, _headers: &Headers) -> Result<HttpResponse, String> {
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

    fn client(t: FakeTransport) -> ApiClient {
        ApiClient::jev(Arc::new(t), "k", "jev-1.13.0".into())
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
        assert_eq!(e.kind, AskErrorKind::Setup);
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn unknown_model_is_a_setup_error_not_a_split() {
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let c = client(FakeTransport::new(move |_, _| {
            n2.fetch_add(1, Ordering::SeqCst);
            Ok(HttpResponse {
                status: 400,
                // Recorded from the live API (2026-09-23).
                body: r#"{"detail":{"error_type":"api_usage_error","message":"Unknown model: jev-0.0.1"}}"#
                    .into(),
                retry_after: None,
            })
        }));
        let qs: BTreeMap<String, Question> = (0..4).map(|i| (format!("q{i}"), noul("x"))).collect();
        let e = c.ask_splitting(&json!("s"), &qs).unwrap_err();
        assert_eq!(e.kind, AskErrorKind::Setup);
        assert_eq!(n.load(Ordering::SeqCst), 1);
        assert_eq!(c.spend().splits, 0);
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
    fn list_models_reads_the_live_envelope() {
        // Recorded from the live API (2026-09-23): pinned versions are
        // not listed, only the moving aliases.
        let c = client(FakeTransport::new(|_, _| {
            Ok(ok(&json!({"models": [
                {"name": "jev-latest", "description": "latest", "release_date": "2026-09-10T18:38:01+00:00"},
                {"name": "jev-preview", "description": "preview", "release_date": "2026-09-10T18:39:06+00:00"}
            ]})))
        }));
        let models = c.list_models().unwrap();
        assert_eq!(models, vec!["jev-latest", "jev-preview"]);
        assert!(!model_listed(&models, "jev-1.13.0"));
        assert!(model_listed(&models, "jev-latest"));
    }

    #[test]
    fn list_models_accepts_other_envelopes() {
        assert_eq!(
            model_ids(&json!({"data": [{"id": "jev-1.13.0"}, {"id": "jev-latest"}]})),
            vec!["jev-1.13.0", "jev-latest"]
        );
        assert_eq!(model_ids(&json!(["b", "a"])), vec!["a", "b"]);
    }

    #[test]
    fn backoff_honours_retry_after_and_caps() {
        assert_eq!(backoff(1, Some("2")), Duration::from_secs(2));
        assert!(backoff(10, None) <= Duration::from_secs(20));
        assert!(rate_limit_wait(30, None) <= Duration::from_secs(5));
    }
}
