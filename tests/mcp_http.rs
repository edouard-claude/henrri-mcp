//! The Streamable HTTP transport and the webhook receiver, end to end.
//!
//! The binary is started with `--http` on a free port, behind a bearer token,
//! with a fake Henrri. A signed delivery posted to `/webhooks` must show up in
//! `list_received_webhook_events` over MCP; an unsigned one must not.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test that cannot fail loudly is not a test"
)]

mod common;

use std::{
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use common::{Fake, WEBHOOK_SECRET, env};
use serde_json::{Value, json};
use tokio::process::{Child, Command};

const TOKEN: &str = "http-test-token";

struct Http {
    _child: Child,
    base: String,
    client: reqwest::Client,
    session: Option<String>,
    next_id: i64,
}

impl Http {
    async fn start(fake: &Fake) -> Self {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut command = Command::new(env!("CARGO_BIN_EXE_mcp-henrri"));
        command
            .env_clear()
            .env("HENRRI_LOG", "warn")
            .env("HENRRI_HTTP_TOKEN", TOKEN)
            .args(["--http", &format!("127.0.0.1:{port}")])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        for (key, value) in env(fake) {
            command.env(key, value);
        }
        let child = command.spawn().expect("the binary runs");
        let base = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::new();
        for _ in 0..100 {
            if client
                .get(format!("{base}/healthz"))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Self {
            _child: child,
            base,
            client,
            session: None,
            next_id: 0,
        }
    }

    /// One JSON-RPC exchange over Streamable HTTP; the answer may come as JSON
    /// or as a one-event SSE stream.
    async fn rpc(&mut self, method: &str, params: Value, notification: bool) -> Option<Value> {
        let mut message = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        if !notification {
            self.next_id += 1;
            message["id"] = json!(self.next_id);
        }
        let mut request = self
            .client
            .post(format!("{}/mcp", self.base))
            .bearer_auth(TOKEN)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-11-25")
            .json(&message);
        if let Some(session) = &self.session {
            request = request.header("mcp-session-id", session);
        }
        let response = request.send().await.unwrap();
        assert!(
            response.status().is_success(),
            "{method}: {}",
            response.status()
        );
        if let Some(session) = response.headers().get("mcp-session-id") {
            self.session = Some(session.to_str().unwrap().to_owned());
        }
        if notification {
            return None;
        }
        let text = response.text().await.unwrap();
        let payload = text
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(str::trim)
            .find(|l| l.contains("\"id\""))
            .map_or_else(|| text.clone(), str::to_owned);
        Some(serde_json::from_str(&payload).unwrap_or_else(|e| panic!("{payload}: {e}")))
    }

    async fn initialize(&mut self) {
        self.rpc(
            "initialize",
            json!({"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}),
            false,
        )
        .await;
        self.rpc("notifications/initialized", json!({}), true).await;
    }

    async fn deliver(&self, body: &str, signature: &str, timestamp: &str) -> reqwest::StatusCode {
        self.client
            .post(format!("{}/webhooks", self.base))
            .header("content-type", "application/json")
            .header("x-henrri-webhook-signature", signature)
            .header("x-henrri-webhook-timestamp", timestamp)
            .header(
                "x-henrri-webhook-delivery",
                "4734794d-065d-4879-83f4-6760416e84d7",
            )
            .body(body.to_owned())
            .send()
            .await
            .unwrap()
            .status()
    }
}

#[tokio::test]
async fn the_mcp_endpoint_requires_the_bearer_token_but_health_does_not() {
    let fake = Fake::start().await;
    let http = Http::start(&fake).await;
    let health = http
        .client
        .get(format!("{}/healthz", http.base))
        .send()
        .await
        .unwrap();
    assert!(health.status().is_success());
    let anonymous = http
        .client
        .post(format!("{}/mcp", http.base))
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert!(anonymous.headers().contains_key("www-authenticate"));
}

#[tokio::test]
async fn a_signed_delivery_is_received_and_an_unsigned_one_refused() {
    let fake = Fake::start().await;
    let mut http = Http::start(&fake).await;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let body = r#"{"event":"document.finalized","date":"2026-11-09T18:00:00+01:00","id":9632,"companyId":947,"delivery":"4734794d-065d-4879-83f4-6760416e84d7"}"#;

    let forged = http.deliver(body, "sha256=00", &now).await;
    assert_eq!(forged, reqwest::StatusCode::UNAUTHORIZED);
    let wrong_secret = mcp_henrri::webhooks::sign("not-the-secret", &now, body.as_bytes());
    assert_eq!(
        http.deliver(body, &wrong_secret, &now).await,
        reqwest::StatusCode::UNAUTHORIZED
    );

    let signature = mcp_henrri::webhooks::sign(WEBHOOK_SECRET, &now, body.as_bytes());
    assert!(http.deliver(body, &signature, &now).await.is_success());
    assert!(
        http.deliver(body, &signature, &now).await.is_success(),
        "a duplicate is acknowledged too"
    );

    http.initialize().await;
    let answer = http
        .rpc(
            "tools/call",
            json!({"name": "list_received_webhook_events", "arguments": {}}),
            false,
        )
        .await
        .unwrap();
    let result = &answer["result"]["structuredContent"];
    assert_eq!(result["returned"], 1, "{answer}");
    assert_eq!(result["events"][0]["event"], "document.finalized");
    assert_eq!(result["events"][0]["id"], 9632);
}
