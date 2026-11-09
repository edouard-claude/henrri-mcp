//! The protocol, end to end, over stdio.
//!
//! These tests spawn the real binary, speak JSON-RPC to its stdin and read its
//! stdout, with a fake Henrri behind it. If the handshake, the tool list, the
//! resources, the prompts or a tool call ever stop being what the MCP
//! specification asks for, this file fails.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test that cannot fail loudly is not a test"
)]

mod common;

use std::process::Stdio;

use common::{CUSTOMER_ID, Fake, env};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
};

/// A connected MCP client, over the binary's stdio.
struct Mcp {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    next_id: i64,
}

impl Mcp {
    /// Spawn the server and complete the handshake.
    async fn start(vars: &[(String, String)]) -> (Self, Value) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mcp-henrri"));
        command
            .env_clear()
            .env("HENRRI_LOG", "warn")
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        for (key, value) in vars {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("the binary runs");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let mut mcp = Self {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            next_id: 0,
        };
        let initialized = mcp
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": { "name": "mcp-henrri-tests", "version": "0" }
                }),
            )
            .await;
        mcp.notify("notifications/initialized", json!({})).await;
        (mcp, initialized)
    }

    /// Send a request and wait for the answer carrying its id.
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.write(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        loop {
            let line =
                tokio::time::timeout(std::time::Duration::from_secs(20), self.lines.next_line())
                    .await
                    .expect("the server answers within 20 s")
                    .expect("stdout is readable")
                    .expect("the server is still running");
            let message: Value = serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("stdout carries only JSON-RPC, got {line:?}: {e}"));
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                return message;
            }
        }
    }

    /// Call a tool and return its result.
    async fn call(&mut self, name: &str, arguments: Value) -> Value {
        let answer = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await;
        answer
            .get("result")
            .cloned()
            .unwrap_or_else(|| panic!("{name} answered no result: {answer}"))
    }

    async fn notify(&mut self, method: &str, params: Value) {
        self.write(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await;
    }

    async fn write(&mut self, message: Value) {
        let mut line = serde_json::to_vec(&message).unwrap();
        line.push(b'\n');
        self.stdin.write_all(&line).await.unwrap();
        self.stdin.flush().await.unwrap();
    }

    async fn stop(mut self) {
        drop(self.stdin);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), self.child.wait()).await;
    }
}

#[tokio::test]
async fn the_handshake_advertises_tools_resources_prompts_and_instructions() {
    let fake = Fake::start().await;
    let (mcp, init) = Mcp::start(&env(&fake)).await;
    let result = &init["result"];
    assert_eq!(result["protocolVersion"], "2025-11-25");
    assert_eq!(result["serverInfo"]["name"], "henrri");
    for capability in ["tools", "resources", "prompts"] {
        assert!(
            result["capabilities"].get(capability).is_some(),
            "{capability}"
        );
    }
    let instructions = result["instructions"].as_str().unwrap();
    assert!(instructions.contains("confirm: true"));
    assert!(instructions.contains("euros"));
    mcp.stop().await;
}

#[tokio::test]
async fn every_tool_is_listed_with_input_and_output_schemas() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake)).await;
    let list = mcp.request("tools/list", json!({})).await;
    let tools = list["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 89);
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        assert_eq!(tool["inputSchema"]["type"], "object", "{name}");
        assert_eq!(tool["outputSchema"]["type"], "object", "{name}");
        assert!(tool["annotations"]["title"].is_string(), "{name}");
    }
    let update = tools
        .iter()
        .find(|t| t["name"] == "update_customer")
        .unwrap();
    let properties = update["inputSchema"]["properties"].as_object().unwrap();
    assert!(properties.contains_key("customer_id"));
    assert!(properties.contains_key("days_number_before_payment_reminder_level1"));
    assert!(properties.contains_key("type"));
    mcp.stop().await;
}

#[tokio::test]
async fn a_tool_call_returns_structured_content_and_a_failure_is_flagged() {
    let fake = Fake::start().await;
    fake.fail(
        "GET",
        "/v1/customers/999999",
        404,
        json!({"title": "Customer not found", "status": 404, "detail": "The id provided does not match any customer", "traceId": "00-x-01"}),
    )
    .await;
    let (mut mcp, _) = Mcp::start(&env(&fake)).await;

    let whoami = mcp.call("whoami", json!({})).await;
    assert_eq!(whoami["isError"], false);
    assert_eq!(
        whoami["structuredContent"]["company"]["name"],
        "Atelier Test"
    );

    let listing = mcp
        .call("list_customers", json!({"search": "client"}))
        .await;
    assert_eq!(listing["structuredContent"]["returned"], 1);
    assert_eq!(listing["structuredContent"]["items"][0]["id"], CUSTOMER_ID);

    let missing = mcp
        .call("get_customer", json!({"customer_id": 999_999}))
        .await;
    assert_eq!(missing["isError"], true);
    let text = missing["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("Customer not found") && text.contains("00-x-01"),
        "{text}"
    );

    let unconfirmed = mcp
        .call("delete_customer", json!({"customer_id": CUSTOMER_ID}))
        .await;
    assert_eq!(unconfirmed["isError"], true);
    assert!(
        unconfirmed["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("MON CLIENT PRO"),
        "the refusal names the customer"
    );
    assert!(
        fake.requests_to("DELETE", &format!("/v1/customers/{CUSTOMER_ID}"))
            .await
            .is_empty()
    );
    mcp.stop().await;
}

#[tokio::test]
async fn resources_and_prompts_are_served() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake)).await;
    let resources = mcp.request("resources/list", json!({})).await;
    let uris: Vec<&str> = resources["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["uri"].as_str())
        .collect();
    assert!(uris.contains(&"henrri://me") && uris.contains(&"henrri://webhooks/events"));
    let templates = mcp.request("resources/templates/list", json!({})).await;
    assert_eq!(
        templates["result"]["resourceTemplates"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let me = mcp
        .request("resources/read", json!({"uri": "henrri://me"}))
        .await;
    let text = me["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains("Atelier Test"));
    let events = mcp
        .request("resources/read", json!({"uri": "henrri://webhooks/events"}))
        .await;
    assert!(
        events["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("document.finalized")
    );

    let prompts = mcp.request("prompts/list", json!({})).await;
    assert_eq!(prompts["result"]["prompts"].as_array().unwrap().len(), 4);
    let prompt = mcp
        .request(
            "prompts/get",
            json!({"name": "draft_invoice", "arguments": {"customer": "Atelier Martin", "what": "3 jours à 600 €"}}),
        )
        .await;
    let text = prompt["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(text.contains("Atelier Martin") && text.contains("confirm: true"));
    mcp.stop().await;
}

#[tokio::test]
async fn read_only_mode_lists_no_write_tool() {
    let fake = Fake::start().await;
    let mut vars = env(&fake);
    vars.push(("HENRRI_READ_ONLY".into(), "1".into()));
    vars.push(("HENRRI_TOOLSETS".into(), "documents,payments".into()));
    let (mut mcp, _) = Mcp::start(&vars).await;
    let list = mcp.request("tools/list", json!({})).await;
    let tools = list["result"]["tools"].as_array().unwrap();
    assert_ne!(tools.len(), 0);
    for tool in tools {
        assert_eq!(
            tool["annotations"]["readOnlyHint"], true,
            "{}",
            tool["name"]
        );
        assert!(
            !tool["name"].as_str().unwrap().contains("customer"),
            "{}",
            tool["name"]
        );
    }
    let refused = mcp
        .request(
            "tools/call",
            json!({"name": "finalize_document", "arguments": {"document_id": 1, "confirm": true}}),
        )
        .await;
    assert!(refused.get("error").is_some() || refused["result"]["isError"] == true);
    mcp.stop().await;
}
