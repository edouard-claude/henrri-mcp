//! The one error type for everything that can go wrong against Henrri, and
//! its two translations: into a tool result a model can read and act on, and
//! into an MCP protocol error for the few places that need one.

use rmcp::{
    ErrorData,
    handler::server::tool::IntoCallToolResult,
    model::{CallToolResponse, CallToolResult, ContentBlock},
};
use serde::Serialize;
use serde_json::{Value, json};

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can fail between the environment and the Henrri API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The environment is missing something, or holds something unusable.
    #[error("configuration: {0}")]
    Config(String),

    /// The caller passed something the server refuses before any HTTP call.
    #[error("{0}")]
    Invalid(String),

    /// Getting an access token failed.
    #[error("authentication: {0}")]
    Auth(String),

    /// The transport failed: DNS, TLS, timeout, connection reset.
    #[error("transport: {0}")]
    Transport(String),

    /// Henrri answered with a 4xx or 5xx.
    #[error("{}", problem.headline())]
    Api {
        /// What Henrri said, read from its RFC 9457 problem document.
        problem: Box<Problem>,
    },

    /// The response was a 2xx but not the JSON this client expected.
    #[error("decoding {endpoint}: {source}")]
    Decode {
        /// The endpoint whose response could not be decoded.
        endpoint: String,
        /// The underlying serde error.
        #[source]
        source: serde_json::Error,
    },

    /// Reading or writing a local file failed.
    #[error("{path}: {source}")]
    Io {
        /// The path involved.
        path: String,
        /// The underlying IO error.
        #[source]
        source: std::io::Error,
    },
}

/// An API failure, as Henrri reports it: an RFC 9457 problem document, plus
/// its `errorCode`, `help` and `traceId` extensions.
#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// The method and path that failed, for instance `POST /v1/documents/9/finalize`.
    pub endpoint: String,
    /// The HTTP status code.
    pub status: u16,
    /// The stable business code (`ALREADY_FINALIZED`, `QUOTA_EXCEEDED`…), when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// The stable title of the problem.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What went wrong in this occurrence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The invalid fields of a validation error, one sentence each.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
    /// Extra help Henrri gives on some business errors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<Value>,
    /// The identifier of the call, to give to Henrri's support.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    /// What to do about it, when this server knows better than the message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl Problem {
    /// One line: endpoint, status, code, and what was said.
    pub fn headline(&self) -> String {
        let mut line = format!("{} → HTTP {}", self.endpoint, self.status);
        if let Some(code) = &self.error_code {
            line.push(' ');
            line.push_str(code);
        }
        let said: Vec<&str> = [self.title.as_deref(), self.detail.as_deref()]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect();
        if !said.is_empty() {
            line.push_str(": ");
            line.push_str(&said.join(" — "));
        }
        if !self.errors.is_empty() {
            line.push_str(" [");
            line.push_str(&self.errors.join("; "));
            line.push(']');
        }
        if let Some(hint) = &self.hint {
            line.push_str(if line.ends_with('.') { " " } else { ". " });
            line.push_str(hint);
        }
        line
    }
}

impl Error {
    /// Shorthand for [`Error::Invalid`].
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    /// Shorthand for [`Error::Config`].
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    /// The HTTP status, when this is an API error.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api { problem } => Some(problem.status),
            _ => None,
        }
    }

    /// The business error code, when Henrri gave one.
    pub fn error_code(&self) -> Option<&str> {
        match self {
            Self::Api { problem } => problem.error_code.as_deref(),
            _ => None,
        }
    }

    /// A short machine-readable kind, so a model can branch without parsing prose.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Config(_) => "configuration",
            Self::Invalid(_) => "invalid_arguments",
            Self::Auth(_) => "authentication",
            Self::Transport(_) => "transport",
            Self::Api { .. } => "henrri_api",
            Self::Decode { .. } => "decoding",
            Self::Io { .. } => "io",
        }
    }

    /// The error as the JSON a tool result carries.
    pub fn to_json(&self) -> Value {
        match self {
            Self::Api { problem } => json!({
                "error": self.kind(),
                "message": self.to_string(),
                "problem": problem,
            }),
            _ => json!({ "error": self.kind(), "message": self.to_string() }),
        }
    }
}

/// A tool failure, reported inside the tool result with `isError: true`.
///
/// The MCP specification keeps protocol errors for a malformed call. An API
/// refusal, a business rule or a missing confirmation is something the model
/// must read and react to, so it travels as the tool's own result, with the
/// problem document attached.
#[derive(Debug)]
pub struct ToolError(pub Error);

impl From<Error> for ToolError {
    fn from(err: Error) -> Self {
        Self(err)
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl IntoCallToolResult for ToolError {
    fn into_call_tool_result(self) -> std::result::Result<CallToolResponse, ErrorData> {
        let body =
            serde_json::to_string_pretty(&self.0.to_json()).unwrap_or_else(|_| self.0.to_string());
        Ok(CallToolResult::error(vec![ContentBlock::text(body)]).into())
    }
}

/// Translate a crate error into an MCP protocol error, for resources and
/// prompts, which have no `isError` channel.
impl From<Error> for ErrorData {
    fn from(err: Error) -> Self {
        let message = err.to_string();
        let data = Some(err.to_json());
        match err {
            Error::Invalid(_) => ErrorData::invalid_params(message, data),
            Error::Api { ref problem } if matches!(problem.status, 400 | 404 | 409 | 422) => {
                ErrorData::invalid_params(message, data)
            }
            _ => ErrorData::internal_error(message, data),
        }
    }
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

    fn business() -> Error {
        Error::Api {
            problem: Box::new(Problem {
                endpoint: "POST /v1/documents/9/finalize".into(),
                status: 400,
                error_code: Some("ALREADY_FINALIZED".into()),
                title: Some("Document already finalized".into()),
                detail: Some("The document already has a definitive number.".into()),
                trace_id: Some("00-abc-01".into()),
                ..Problem::default()
            }),
        }
    }

    #[test]
    fn the_headline_names_endpoint_status_code_and_detail() {
        let line = business().to_string();
        assert!(line.starts_with("POST /v1/documents/9/finalize → HTTP 400 ALREADY_FINALIZED"));
        assert!(line.contains("definitive number"), "{line}");
    }

    #[test]
    fn an_api_failure_is_a_tool_result_flagged_as_error() {
        let response = ToolError(business()).into_call_tool_result().unwrap();
        let CallToolResponse::Complete(result) = response else {
            panic!("expected a complete result");
        };
        assert_eq!(result.is_error, Some(true));
        let text = serde_json::to_string(&result.content).unwrap();
        assert!(text.contains("ALREADY_FINALIZED"), "{text}");
        assert!(
            text.contains("00-abc-01"),
            "the trace id reaches the model: {text}"
        );
    }

    #[test]
    fn a_business_error_is_invalid_params_for_resources() {
        let data: ErrorData = business().into();
        assert_eq!(data.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        let data: ErrorData = Error::Transport("reset".into()).into();
        assert_eq!(data.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
    }
}
