//! Against the real Henrri sandbox, when asked to.
//!
//! Skipped unless `HENRRI_SANDBOX_TESTS=1`, with sandbox credentials in the
//! environment (`HENRRI_USERNAME` / `HENRRI_PASSWORD`, or a client id and
//! secret). It reads the reference lists, then builds a draft quote with lines
//! and deletes it: nothing is finalized, no credit is spent.
//!
//! ```bash
//! set -a; source .env; set +a
//! HENRRI_SANDBOX_TESTS=1 cargo test --test sandbox -- --nocapture
//! ```

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout,
    reason = "a test that cannot fail loudly is not a test"
)]

use mcp_henrri::{Config, HenrriServer, config::Environment};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};

fn sandbox() -> Option<HenrriServer> {
    if std::env::var("HENRRI_SANDBOX_TESTS").as_deref() != Ok("1") {
        println!("skipped: set HENRRI_SANDBOX_TESTS=1 and sandbox credentials to run");
        return None;
    }
    let config = Config::from_env().expect("sandbox credentials in the environment");
    assert_eq!(
        config.environment,
        Environment::Sandbox,
        "these tests write: point them at the sandbox (HENRRI_ENVIRONMENT=sandbox)"
    );
    Some(HenrriServer::new(config).unwrap())
}

fn args<T: serde::de::DeserializeOwned>(value: Value) -> Parameters<T> {
    Parameters(serde_json::from_value(value).unwrap())
}

#[tokio::test]
async fn the_sandbox_answers_the_read_surface() {
    let Some(server) = sandbox() else { return };
    assert!(server.ping().await.unwrap().0.ok);
    let me = server.whoami().await.unwrap().0;
    assert!(me.0["company"]["id"].as_u64().is_some());
    let types = server.list_document_types().await.unwrap().0;
    assert!(types.items.iter().any(|t| t["documentKind"] == "quotation"));
    assert_ne!(server.list_payment_modes().await.unwrap().0.items.len(), 0);
    assert_ne!(
        server
            .list_document_line_types()
            .await
            .unwrap()
            .0
            .items
            .len(),
        0
    );
    let usage = server.get_api_usage().await.unwrap().0;
    assert!(usage.usage.credits_used.is_some());
}

#[tokio::test]
async fn a_draft_quote_is_built_and_deleted() {
    let Some(server) = sandbox() else { return };
    let quote_type = server
        .list_document_types()
        .await
        .unwrap()
        .0
        .items
        .into_iter()
        .find(|t| t["documentKind"] == "quotation")
        .and_then(|t| t["id"].as_u64())
        .unwrap();
    let category = server
        .list_item_categories(args(json!({})))
        .await
        .unwrap()
        .0
        .items[0]["id"]
        .as_u64()
        .unwrap();
    let customer = server
        .create_customer(args(json!({
            "name": "mcp-henrri sandbox test", "type": "professional",
            "contacts": [{"last_name": "Test"}]
        })))
        .await
        .unwrap()
        .0;
    let customer_id = customer.0["id"].as_u64().unwrap();
    let quote = server
        .create_document(args(json!({
            "document_type_id": quote_type, "customer_id": customer_id, "title": "sandbox test"
        })))
        .await
        .unwrap()
        .0;
    let quote_id = quote.0["id"].as_u64().unwrap();
    let line = server
        .add_document_line(args(json!({
            "document_id": quote_id, "kind": "item", "quantity": 2,
            "item": {"description": "sandbox test item", "vat_percent": 20,
                     "selling_price_without_tax": 100, "item_category_id": category}
        })))
        .await
        .unwrap()
        .0;
    let line_id = line.0["id"].as_u64().unwrap();
    let updated = server
        .update_document_line(args(
            json!({"document_id": quote_id, "line_id": line_id, "quantity": 3}),
        ))
        .await
        .unwrap()
        .0;
    assert_eq!(updated.0["quantity"], 3.0);
    let document = server
        .get_document(args(json!({"document_id": quote_id})))
        .await
        .unwrap()
        .0;
    assert_eq!(document.0["priceBeforeTax"], 300.0);
    server
        .delete_document(args(json!({"document_id": quote_id, "confirm": true})))
        .await
        .unwrap();
    server
        .delete_customer(args(json!({"customer_id": customer_id, "confirm": true})))
        .await
        .unwrap();
}
