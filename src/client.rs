//! The Henrri v1 client: REST over HTTP, with the API's manners respected.
//!
//! Four things are handled here, once, so no tool has to think about them:
//!
//! * **The token lifecycle.** Henrri access tokens live ten minutes. The client
//!   asks for one lazily, renews it a minute before it expires, and retries a
//!   request once on a 401, so an expired or revoked token never surfaces as a
//!   tool failure. A developer client is renewed through
//!   `POST /v1/users/refresh-token`; an API user is simply granted again, since
//!   `/v1/users/token` refuses the `refresh_token` grant.
//! * **Retries, but only where they are safe.** The per-minute rate limiter's
//!   429, 503, and the `409 FINALIZATION_IN_PROGRESS` that `finalize` answers
//!   with a `Retry-After` are retried with backoff. The monthly credit quota's
//!   429 (`QUOTA_EXCEEDED`) is not: retrying cannot help until next month. A
//!   `POST` is never resent after its request may have reached the server.
//! * **Henrri's transport quirks.** Its IIS front refuses a `POST` without a
//!   `Content-Length` (HTTP 411), so a body-less `POST` carries an explicit empty
//!   one.
//! * **Errors and credits.** Every failure is read from its RFC 9457 problem
//!   document into one [`Error::Api`]; every answer's `X-Credits-*` and
//!   `X-RateLimit-*` headers are kept for the `get_api_usage` tool.

use std::{sync::Mutex as SyncMutex, time::Duration};

use reqwest::{Method, StatusCode, header};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::{sync::Mutex, time::Instant};

use crate::{
    config::{Config, Credentials},
    error::{Error, Problem, Result},
};

/// The `User-Agent` every request carries.
pub const USER_AGENT: &str = concat!("mcp-henrri/", env!("CARGO_PKG_VERSION"));

/// How long before its announced expiry a token is renewed.
const RENEW_MARGIN: Duration = Duration::from_secs(60);

/// The longest a single backoff waits, whatever the server asks.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// A query string under construction: repeated keys are how Henrri reads
/// arrays (`documentTypes=invoice&documentTypes=creditNote`).
pub type Query = Vec<(String, String)>;

/// How long a request may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deadline {
    /// The configured per-request timeout.
    Standard,
    /// The longer timeout of the PDF endpoints, which render on demand.
    Pdf,
}

/// One request to send, before authentication is added.
#[derive(Debug, Clone)]
pub struct Request {
    method: Method,
    path: String,
    query: Query,
    body: Option<Value>,
    accept: &'static str,
    deadline: Deadline,
}

impl Request {
    /// A request for `path`, relative to the API root, for instance `/v1/documents/42`.
    pub fn new(method: Method, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            query: Vec::new(),
            body: None,
            accept: "application/json",
            deadline: Deadline::Standard,
        }
    }

    /// A `GET`.
    pub fn get(path: impl Into<String>) -> Self {
        Self::new(Method::GET, path)
    }

    /// A `POST`.
    pub fn post(path: impl Into<String>) -> Self {
        Self::new(Method::POST, path)
    }

    /// A `PUT`.
    pub fn put(path: impl Into<String>) -> Self {
        Self::new(Method::PUT, path)
    }

    /// A `DELETE`.
    pub fn delete(path: impl Into<String>) -> Self {
        Self::new(Method::DELETE, path)
    }

    /// Add the query string.
    #[must_use]
    pub fn query(mut self, query: Query) -> Self {
        self.query = query;
        self
    }

    /// Add a JSON body.
    #[must_use]
    pub fn json(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    /// Ask for something other than JSON back, with the PDF deadline.
    #[must_use]
    pub fn binary(mut self, accept: &'static str) -> Self {
        self.accept = accept;
        self.deadline = Deadline::Pdf;
        self
    }

    /// Use the PDF deadline while still expecting JSON.
    #[must_use]
    pub fn slow(mut self) -> Self {
        self.deadline = Deadline::Pdf;
        self
    }

    /// `GET /v1/documents/42`, for messages.
    pub fn label(&self) -> String {
        format!("{} {}", self.method, self.path)
    }

    /// True when sending the same request twice cannot do anything twice.
    fn is_idempotent(&self) -> bool {
        self.method != Method::POST
    }
}

/// What a request answered, before decoding.
#[derive(Debug, Clone)]
pub struct Response {
    /// The body.
    pub bytes: Vec<u8>,
    /// The media type, as the server labelled it.
    pub content_type: String,
    /// The file name the server suggested, from `Content-Disposition`.
    pub file_name: Option<String>,
}

/// The credit and rate-limit counters Henrri attaches to every answer.
#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
pub struct Usage {
    /// Credits consumed this calendar month (`X-Credits-Used`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits_used: Option<String>,
    /// The plan's monthly quota, or `unlimited` (`X-Credits-Limit`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits_limit: Option<String>,
    /// Credits left this month, or `unlimited` (`X-Credits-Remaining`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits_remaining: Option<String>,
    /// Requests allowed per window by the rate limiter (`X-RateLimit-Limit`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<String>,
    /// Requests left in the current window (`X-RateLimit-Remaining`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_remaining: Option<String>,
    /// When the window resets, unix seconds (`X-RateLimit-Reset`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_reset: Option<String>,
}

impl Usage {
    fn read(headers: &header::HeaderMap) -> Self {
        let get = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        Self {
            credits_used: get("x-credits-used"),
            credits_limit: get("x-credits-limit"),
            credits_remaining: get("x-credits-remaining"),
            rate_limit: get("x-ratelimit-limit"),
            rate_limit_remaining: get("x-ratelimit-remaining"),
            rate_limit_reset: get("x-ratelimit-reset"),
        }
    }

    fn is_empty(&self) -> bool {
        self.credits_used.is_none() && self.credits_limit.is_none() && self.rate_limit.is_none()
    }
}

/// A client for one Henrri account.
#[derive(Debug)]
pub struct Henrri {
    http: reqwest::Client,
    config: Config,
    /// Held across the token request on purpose: concurrent tool calls wait for
    /// the one grant in flight instead of each asking Henrri for its own.
    token: Mutex<Option<Token>>,
    /// The last counters seen. A plain mutex: it is held for a copy, never
    /// across an await.
    usage: SyncMutex<Usage>,
}

#[derive(Debug, Clone)]
struct Token {
    access: String,
    refresh: Option<String>,
    expires_at: Option<Instant>,
}

impl Token {
    fn is_fresh(&self) -> bool {
        self.expires_at
            .is_none_or(|at| Instant::now() + RENEW_MARGIN < at)
    }
}

/// What the token endpoints answer. The OAuth endpoint and the quickstart use
/// snake case; the `TokenResponse` schema documents camel case. Both are read.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    #[serde(alias = "accessToken")]
    access_token: Option<String>,
    #[serde(alias = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(alias = "expiresIn")]
    expires_in: Option<u64>,
    #[serde(alias = "errorDescription")]
    error_description: Option<String>,
    error: Option<String>,
}

impl Henrri {
    /// Build a client from a configuration. No request is sent until the first
    /// call.
    pub fn new(config: Config) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| Error::Transport(describe(&e)))?;
        let token = match &config.credentials {
            Credentials::Token(token) => Some(Token {
                access: token.expose().to_owned(),
                refresh: None,
                expires_at: None,
            }),
            Credentials::Client { .. } | Credentials::Password { .. } => None,
        };
        Ok(Self {
            http,
            config,
            token: Mutex::new(token),
            usage: SyncMutex::new(Usage::default()),
        })
    }

    /// The configuration this client was built from.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The credit and rate-limit counters of the last answer, if any came yet.
    pub fn usage(&self) -> Usage {
        self.usage
            .lock()
            .map(|usage| usage.clone())
            .unwrap_or_default()
    }

    /// Send a request and decode its JSON answer. An empty body reads as
    /// `null`, which is what Henrri's `204` and some `200` answers carry.
    pub async fn call<R: DeserializeOwned>(&self, request: Request) -> Result<R> {
        let label = request.label();
        let response = self.send(request).await?;
        let bytes = if response.bytes.iter().all(u8::is_ascii_whitespace) {
            b"null".as_slice()
        } else {
            response.bytes.as_slice()
        };
        serde_json::from_slice(bytes).map_err(|source| Error::Decode {
            endpoint: label,
            source,
        })
    }

    /// Send a request and return its raw body, whatever its type.
    pub async fn send(&self, request: Request) -> Result<Response> {
        let label = request.label();
        let url = format!("{}{}", self.config.base_url, request.path);
        let timeout = match request.deadline {
            Deadline::Standard => self.config.timeout,
            Deadline::Pdf => self.config.pdf_timeout,
        };
        let mut attempt = 0;
        let mut reauthenticated = false;
        loop {
            attempt += 1;
            let token = self.access_token().await?;
            tracing::debug!(endpoint = %label, attempt, "henrri request");
            let mut builder = self
                .http
                .request(request.method.clone(), &url)
                .timeout(timeout)
                .bearer_auth(&token)
                .header(header::ACCEPT, request.accept);
            if !request.query.is_empty() {
                builder = builder.query(&request.query);
            }
            builder = match &request.body {
                Some(body) => builder.json(body),
                // IIS answers 411 to a body-less POST without Content-Length.
                None if request.method == Method::POST || request.method == Method::PUT => {
                    builder.header(header::CONTENT_LENGTH, "0").body(Vec::new())
                }
                None => builder,
            };
            let response = match builder.send().await {
                Ok(response) => response,
                Err(e) => {
                    // A POST that may have reached the server is not resent: it
                    // could create a second document or a second payment.
                    let unsent = e.is_connect();
                    let err = Error::Transport(describe(&e));
                    if attempt <= self.config.max_retries && (unsent || request.is_idempotent()) {
                        tracing::warn!(endpoint = %label, attempt, error = %err, "retrying");
                        backoff(attempt, None).await;
                        continue;
                    }
                    return Err(err);
                }
            };
            let status = response.status();
            let headers = response.headers().clone();
            self.record_usage(&headers);
            let bytes = response
                .bytes()
                .await
                .map_err(|e| Error::Transport(describe(&e)))?
                .to_vec();
            if status.is_success() {
                return Ok(Response {
                    content_type: header_str(&headers, header::CONTENT_TYPE)
                        .unwrap_or("application/octet-stream")
                        .to_owned(),
                    file_name: header_str(&headers, header::CONTENT_DISPOSITION)
                        .and_then(file_name),
                    bytes,
                });
            }
            // A token can be revoked before it expires; get a new one, once.
            if status == StatusCode::UNAUTHORIZED
                && !reauthenticated
                && !matches!(self.config.credentials, Credentials::Token(_))
            {
                reauthenticated = true;
                *self.token.lock().await = None;
                continue;
            }
            let err = api_error(&label, status, &bytes);
            if attempt <= self.config.max_retries && should_retry(&request, &err) {
                tracing::warn!(endpoint = %label, attempt, status = status.as_u16(), "retrying");
                backoff(attempt, retry_after(&headers)).await;
                continue;
            }
            return Err(err);
        }
    }

    fn record_usage(&self, headers: &header::HeaderMap) {
        let usage = Usage::read(headers);
        if usage.is_empty() {
            return;
        }
        if let Ok(mut slot) = self.usage.lock() {
            *slot = usage;
        }
    }

    /// A valid access token, obtained or renewed as needed.
    pub async fn access_token(&self) -> Result<String> {
        let mut slot = self.token.lock().await;
        if let Some(token) = slot.as_ref().filter(|t| t.is_fresh()) {
            return Ok(token.access.clone());
        }
        let token = match &self.config.credentials {
            // A static token cannot be renewed; let the API say it expired.
            Credentials::Token(token) => return Ok(token.expose().to_owned()),
            Credentials::Password { username, password } => {
                self.grant(
                    "POST /v1/users/token",
                    self.http
                        .post(format!("{}/v1/users/token", self.config.base_url))
                        .form(&[
                            ("grant_type", "password"),
                            ("username", username.as_str()),
                            ("password", password.expose()),
                        ]),
                )
                .await?
            }
            Credentials::Client {
                client_id,
                client_secret,
            } => {
                let refreshed = match slot.as_ref().and_then(|t| t.refresh.clone()) {
                    Some(refresh) => self
                        .grant(
                            "POST /v1/users/refresh-token",
                            self.http
                                .post(format!("{}/v1/users/refresh-token", self.config.base_url))
                                .json(&json!({
                                    "refreshToken": refresh,
                                    "clientId": client_id,
                                    "clientSecret": client_secret.expose(),
                                })),
                        )
                        .await
                        .inspect_err(|err| {
                            tracing::debug!(%err, "refresh refused, authenticating again");
                        })
                        .ok(),
                    None => None,
                };
                match refreshed {
                    Some(token) => token,
                    None => {
                        self.grant(
                            "POST /v1/users/authenticate",
                            self.http
                                .post(format!("{}/v1/users/authenticate", self.config.base_url))
                                .json(&json!({
                                    "clientId": client_id,
                                    "clientSecret": client_secret.expose(),
                                })),
                        )
                        .await?
                    }
                }
            }
        };
        let access = token.access.clone();
        *slot = Some(token);
        Ok(access)
    }

    async fn grant(&self, label: &str, builder: reqwest::RequestBuilder) -> Result<Token> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let Some(request) = builder.try_clone() else {
                return Err(Error::Auth(format!("{label}: request cannot be cloned")));
            };
            let response = match request
                .timeout(self.config.timeout)
                .header(header::ACCEPT, "application/json")
                .send()
                .await
            {
                Ok(response) => response,
                Err(e) if attempt <= self.config.max_retries => {
                    tracing::warn!(error = %e, "token endpoint unreachable, retrying");
                    backoff(attempt, None).await;
                    continue;
                }
                Err(e) => return Err(Error::Auth(format!("{label} unreachable: {e}"))),
            };
            let status = response.status();
            let wait = retry_after(response.headers());
            let bytes = response
                .bytes()
                .await
                .map_err(|e| Error::Transport(describe(&e)))?;
            if (status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error())
                && attempt <= self.config.max_retries
            {
                backoff(attempt, wait).await;
                continue;
            }
            let parsed: Option<TokenResponse> = serde_json::from_slice(&bytes).ok();
            let parsed = match parsed {
                Some(TokenResponse {
                    access_token: Some(access),
                    refresh_token,
                    expires_in,
                    ..
                }) if status.is_success() => {
                    return Ok(Token {
                        access,
                        refresh: refresh_token,
                        expires_at: expires_in
                            .map(|secs| Instant::now() + Duration::from_secs(secs)),
                    });
                }
                other => other,
            };
            let said = parsed
                .and_then(|t| t.error_description.or(t.error))
                .unwrap_or_else(|| truncate(String::from_utf8_lossy(&bytes).trim(), 200));
            return Err(Error::Auth(format!(
                "Henrri refused the credentials ({label} → HTTP {}{}). Check HENRRI_USERNAME/HENRRI_PASSWORD or HENRRI_CLIENT_ID/HENRRI_CLIENT_SECRET, and HENRRI_ENVIRONMENT: sandbox accounts do not exist in production, nor the reverse.",
                status.as_u16(),
                if said.is_empty() {
                    String::new()
                } else {
                    format!(": {said}")
                }
            )));
        }
    }
}

/// A reqwest error with its causes, which its own message leaves out
/// ("error sending request" says nothing; "operation timed out" does).
fn describe(err: &reqwest::Error) -> String {
    let mut message = err.to_string();
    let mut source = std::error::Error::source(err);
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !message.contains(&cause_text) {
            message.push_str(": ");
            message.push_str(&cause_text);
        }
        source = cause.source();
    }
    if err.is_timeout() {
        message.push_str(
            " (timed out: raise HENRRI_TIMEOUT_SECS, or HENRRI_PDF_TIMEOUT_SECS for PDFs)",
        );
    }
    message
}

/// Whether a failed request is worth sending again.
fn should_retry(request: &Request, err: &Error) -> bool {
    let Error::Api { problem } = err else {
        return false;
    };
    match (problem.status, problem.error_code.as_deref()) {
        // The monthly credit quota does not reset by waiting a second.
        (429, Some("QUOTA_EXCEEDED")) => false,
        // The per-minute rate limiter, an unavailable service, and another
        // accounting document being finalized (this one is still a draft).
        (429 | 503, _) | (409, Some("FINALIZATION_IN_PROGRESS")) => true,
        // A gateway may have forwarded the request: only resend what is idempotent.
        (502 | 504, _) => request.is_idempotent(),
        _ => false,
    }
}

async fn backoff(attempt: u32, retry_after: Option<Duration>) {
    let wait = retry_after
        .unwrap_or_else(|| Duration::from_millis(500 * u64::from(attempt)))
        .min(MAX_BACKOFF);
    tokio::time::sleep(wait).await;
}

fn header_str(headers: &header::HeaderMap, name: header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn retry_after(headers: &header::HeaderMap) -> Option<Duration> {
    header_str(headers, header::RETRY_AFTER)?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// The `filename` of a `Content-Disposition` header.
fn file_name(disposition: &str) -> Option<String> {
    disposition
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("filename="))
        .map(|name| name.trim_matches('"').to_owned())
        .filter(|name| !name.is_empty())
}

/// Read Henrri's problem document, or fall back to the raw body.
pub(crate) fn api_error(endpoint: &str, status: StatusCode, body: &[u8]) -> Error {
    let parsed: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let text = |key: &str| {
        parsed
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };

    let mut errors = Vec::new();
    if let Some(Value::Object(fields)) = parsed.get("errors") {
        for (field, problems) in fields {
            let list: Vec<&str> = problems
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if !list.is_empty() {
                errors.push(format!("{field}: {}", list.join(" ")));
            }
        }
    }

    let mut problem = Problem {
        endpoint: endpoint.to_owned(),
        status: status.as_u16(),
        // `code` is the pre-October 2026 spelling of the quota error.
        error_code: text("errorCode").or_else(|| text("code")),
        title: text("title"),
        detail: text("detail").or_else(|| text("message")),
        errors,
        help: parsed.get("help").filter(|h| !h.is_null()).cloned(),
        trace_id: text("traceId"),
        hint: None,
    };
    if parsed.is_null() {
        let raw = String::from_utf8_lossy(body);
        let raw = raw.trim();
        // IIS error pages are HTML; their <title> is the useful part.
        let raw = raw
            .split_once("<TITLE>")
            .or_else(|| raw.split_once("<title>"))
            .and_then(|(_, rest)| rest.split_once('<').map(|(t, _)| t))
            .unwrap_or(raw);
        if !raw.is_empty() {
            problem.detail = Some(truncate(raw, 300));
        }
    }
    problem.hint = hint(&problem);
    Error::Api {
        problem: Box::new(problem),
    }
}

/// What to do about a failure, when the message alone does not say it.
fn hint(problem: &Problem) -> Option<String> {
    let text = match (problem.status, problem.error_code.as_deref()) {
        (429, Some("QUOTA_EXCEEDED")) => {
            "the monthly credit quota of the API plan is spent: only finalizing and validating are blocked, reads keep working, and the quota resets on the 1st of the month (UTC)"
        }
        (429, _) => "the per-minute rate limit is reached: wait a little and call again",
        (401, _) => "the access token was refused: check the credentials and HENRRI_ENVIRONMENT",
        (403, _) => {
            "this account's token lacks the scope this endpoint requires (Customer_Write, Document_Write, Payment_Write…): use an API user or client granted it"
        }
        (_, Some("FIRST_ACCOUNTING_DOCUMENT_MONTH")) => {
            "call finalize_document again with confirm_first_of_month: true, after telling the person this is the first accounting document of the month"
        }
        (_, Some("PUT_WITH_CUSTOMER")) => {
            "contacts are managed with add_customer_contact, update_customer_contact and delete_customer_contact"
        }
        (_, Some("DEPOSIT_OPTION_NOT_ENABLED")) => {
            "the advanced deposit management is an option of the Henrri account: ask Henrri's support to enable it"
        }
        (_, Some("TAX_MODE_MISMATCH")) => {
            "read the document's isTaxIncludedMode and set the line's is_tax_included to the same value, or leave it out"
        }
        (500, _) if problem.endpoint.contains("/pdf") => {
            "Henrri could not render this document: retry later, and give the traceId to Henrri's support if it persists"
        }
        _ => return None,
    };
    Some(text.to_owned())
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    let kept: String = value.chars().take(max).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot fail loudly is not a test"
    )]
    use super::*;

    fn problem(err: Error) -> Problem {
        match err {
            Error::Api { problem } => *problem,
            other => panic!("expected an API error, got {other:?}"),
        }
    }

    #[test]
    fn a_business_error_is_read_with_its_code_and_trace() {
        let p = problem(api_error(
            "POST /v1/documents/9/finalize",
            StatusCode::BAD_REQUEST,
            br#"{"type":"https://api.henrri.io/scalar/#tag/businesserrors","title":"Document already finalized","status":400,"detail":"It is locked.","traceId":"00-1-01","errorCode":"ALREADY_FINALIZED"}"#,
        ));
        assert_eq!(p.status, 400);
        assert_eq!(p.error_code.as_deref(), Some("ALREADY_FINALIZED"));
        assert_eq!(p.title.as_deref(), Some("Document already finalized"));
        assert_eq!(p.trace_id.as_deref(), Some("00-1-01"));
    }

    #[test]
    fn validation_errors_list_their_fields() {
        let p = problem(api_error(
            "GET /v1/customers",
            StatusCode::BAD_REQUEST,
            br#"{"title":"One or more validation errors occurred.","status":400,"errors":{"sortBy":["Value nope is not valid. Allowed values are: ClientId, id, date, name."]}}"#,
        ));
        assert_eq!(p.errors.len(), 1);
        assert!(p.errors[0].starts_with("sortBy: "), "{:?}", p.errors);
    }

    #[test]
    fn the_former_quota_shape_is_still_read() {
        let p = problem(api_error(
            "POST /v1/documents/9/finalize",
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"code":"QUOTA_EXCEEDED","message":"You have exceeded your monthly Quota of 200 API calls.","current_usage":201}"#,
        ));
        assert_eq!(p.error_code.as_deref(), Some("QUOTA_EXCEEDED"));
        assert!(p.hint.as_deref().unwrap_or("").contains("1st of the month"));
    }

    #[test]
    fn an_iis_page_is_reduced_to_its_title() {
        let p = problem(api_error(
            "POST /v1/documents/9/pdf/url",
            StatusCode::LENGTH_REQUIRED,
            b"<HTML><HEAD><TITLE>Length Required</TITLE></HEAD><BODY>...</BODY></HTML>",
        ));
        assert_eq!(p.detail.as_deref(), Some("Length Required"));
    }

    #[test]
    fn the_quota_is_never_retried_but_the_rate_limiter_is() {
        let post = Request::post("/v1/documents/9/finalize");
        let quota = api_error(
            "x",
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"errorCode":"QUOTA_EXCEEDED"}"#,
        );
        let limiter = api_error("x", StatusCode::TOO_MANY_REQUESTS, b"");
        let busy = api_error(
            "x",
            StatusCode::CONFLICT,
            br#"{"errorCode":"FINALIZATION_IN_PROGRESS"}"#,
        );
        let gateway = api_error("x", StatusCode::GATEWAY_TIMEOUT, b"");
        assert!(!should_retry(&post, &quota));
        assert!(should_retry(&post, &limiter));
        assert!(should_retry(&post, &busy));
        assert!(
            !should_retry(&post, &gateway),
            "a POST behind a gateway timeout may have run"
        );
        assert!(should_retry(&Request::get("/v1/documents"), &gateway));
    }

    #[test]
    fn content_disposition_names_the_file() {
        assert_eq!(
            file_name(r#"attachment; filename="Facture-2026-001.pdf"; filename*=UTF-8''x"#)
                .as_deref(),
            Some("Facture-2026-001.pdf")
        );
        assert_eq!(file_name("inline"), None);
    }
}
