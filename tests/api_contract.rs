//! The client and the tools against a fake Henrri: what goes on the wire.
//!
//! Each test pins one behaviour the real API forced on this server: the token
//! grant, the 411 on a body-less POST, the PUT that erases what it is not sent,
//! the two kinds of 429, the repeated list keys, the due date ignored on
//! creation. If one of them regresses, the matching test names it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test that cannot fail loudly is not a test"
)]

mod common;

use common::{CUSTOMER_ID, DOCUMENT_ID, Fake, config, customer, page};
use mcp_henrri::{
    HenrriServer,
    client::{Henrri, Request},
    error::Error,
};
use rmcp::handler::server::{tool::IntoCallToolResult, wrapper::Parameters};
use serde_json::{Value, json};
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{method, path, query_param},
};

fn server(fake: &Fake, extra: &[(&str, &str)]) -> HenrriServer {
    HenrriServer::new(config(fake, extra)).unwrap()
}

fn args<T: serde::de::DeserializeOwned>(value: Value) -> Parameters<T> {
    Parameters(serde_json::from_value(value).unwrap())
}

fn body(request: &wiremock::Request) -> Value {
    serde_json::from_slice(&request.body).unwrap()
}

#[tokio::test]
async fn an_api_user_is_granted_once_and_the_token_reused() {
    let fake = Fake::start().await;
    let client = Henrri::new(config(&fake, &[])).unwrap();
    for _ in 0..3 {
        let _: Value = client
            .call(Request::get("/v1/secures/hello-world"))
            .await
            .unwrap();
    }
    let grants = fake.requests_to("POST", "/v1/users/token").await;
    assert_eq!(grants.len(), 1, "one grant for three calls");
    let form = String::from_utf8_lossy(&grants[0].body).into_owned();
    assert!(form.contains("grant_type=password"), "{form}");
    assert!(form.contains("username=me%40example.com"), "{form}");
    let call = &fake.requests_to("GET", "/v1/secures/hello-world").await[0];
    assert_eq!(
        call.headers.get("authorization").unwrap(),
        "Bearer jwt-test"
    );
}

#[tokio::test]
async fn a_developer_client_authenticates_with_json_and_again_after_a_401() {
    let fake = Fake::bare().await;
    Mock::given(method("POST"))
        .and(path("/v1/users/authenticate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "client-jwt", "expires_in": 600, "refresh_token": "r1"
        })))
        .mount(&fake.server)
        .await;
    // The first call is refused once, as a revoked token would be.
    Mock::given(method("GET"))
        .and(path("/v1/users/me"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&fake.server)
        .await;
    fake.mount("GET", "/v1/users/me", json!({"user": {"id": 1}}))
        .await;
    let client = Henrri::new(config(
        &fake,
        &[
            ("HENRRI_CLIENT_ID", "dev-client"),
            ("HENRRI_CLIENT_SECRET", "dev-secret"),
        ],
    ))
    .unwrap();
    let me: Value = client.call(Request::get("/v1/users/me")).await.unwrap();
    assert_eq!(me["user"]["id"], 1);
    let auths = fake.requests_to("POST", "/v1/users/authenticate").await;
    assert_eq!(auths.len(), 2, "authenticated again after the 401");
    assert_eq!(
        body(&auths[0]),
        json!({"clientId": "dev-client", "clientSecret": "dev-secret"})
    );
}

#[tokio::test]
async fn refused_credentials_name_the_variables_to_check() {
    let fake = Fake {
        server: wiremock::MockServer::start().await,
    };
    Mock::given(method("POST"))
        .and(path("/v1/users/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(
            json!({"error": "invalid_grant", "error_description": "invalid_username_or_password"}),
        ))
        .mount(&fake.server)
        .await;
    let client = Henrri::new(config(&fake, &[])).unwrap();
    let err = client.access_token().await.unwrap_err().to_string();
    assert!(err.contains("invalid_username_or_password"), "{err}");
    assert!(err.contains("HENRRI_ENVIRONMENT"), "{err}");
}

#[tokio::test]
async fn a_bodyless_post_carries_a_content_length_for_iis() {
    let fake = Fake::start().await;
    fake.mount(
        "POST",
        &format!("/v1/documents/{DOCUMENT_ID}/transform-to-invoice"),
        json!({"id": 9700, "type": "Facture"}),
    )
    .await;
    let server = server(&fake, &[]);
    let invoice = server
        .transform_quote_to_invoice(args(json!({"document_id": DOCUMENT_ID})))
        .await
        .unwrap();
    assert_eq!(invoice.0.0["id"], 9700);
    let sent = &fake
        .requests_to(
            "POST",
            &format!("/v1/documents/{DOCUMENT_ID}/transform-to-invoice"),
        )
        .await[0];
    assert_eq!(sent.headers.get("content-length").unwrap(), "0");
}

#[tokio::test]
async fn the_rate_limiter_is_retried_but_the_monthly_quota_is_not() {
    let fake = Fake::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/items"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .up_to_n_times(1)
        .mount(&fake.server)
        .await;
    fake.mount("GET", "/v1/items", page(vec![json!({"id": 1})]))
        .await;
    fake.fail(
        "POST",
        &format!("/v1/documents/{DOCUMENT_ID}/finalize"),
        429,
        json!({"status": 429, "errorCode": "QUOTA_EXCEEDED", "detail": "monthly quota reached"}),
    )
    .await;
    let client = Henrri::new(config(&fake, &[])).unwrap();
    let items: Value = client.call(Request::get("/v1/items")).await.unwrap();
    assert_eq!(items["elements"][0]["id"], 1);
    assert_eq!(fake.requests_to("GET", "/v1/items").await.len(), 2);

    let err = client
        .call::<Value>(
            Request::post(format!("/v1/documents/{DOCUMENT_ID}/finalize")).json(json!({})),
        )
        .await
        .unwrap_err();
    assert_eq!(err.error_code(), Some("QUOTA_EXCEEDED"));
    assert!(err.to_string().contains("1st of the month"), "{err}");
    assert_eq!(
        fake.requests_to("POST", &format!("/v1/documents/{DOCUMENT_ID}/finalize"))
            .await
            .len(),
        1,
        "the quota is not retried"
    );
}

#[tokio::test]
async fn a_busy_finalization_is_retried_and_a_gateway_timeout_on_post_is_not() {
    let fake = Fake::start().await;
    let finalize = format!("/v1/documents/{DOCUMENT_ID}/finalize");
    Mock::given(method("POST"))
        .and(path(finalize.as_str()))
        .respond_with(
            ResponseTemplate::new(409)
                .insert_header("retry-after", "0")
                .set_body_json(json!({"status": 409, "errorCode": "FINALIZATION_IN_PROGRESS"})),
        )
        .up_to_n_times(1)
        .mount(&fake.server)
        .await;
    fake.mount(
        "POST",
        &finalize,
        json!({"id": DOCUMENT_ID, "finalized": true}),
    )
    .await;
    let server = server(&fake, &[]);
    let done = server
        .finalize_document(args(json!({"document_id": DOCUMENT_ID, "confirm": true})))
        .await
        .unwrap();
    assert_eq!(done.0.0["finalized"], true);
    assert_eq!(fake.requests_to("POST", &finalize).await.len(), 2);

    let transform = format!("/v1/documents/{DOCUMENT_ID}/transform-to-invoice");
    fake.fail("POST", &transform, 504, json!({})).await;
    assert!(
        server
            .transform_quote_to_invoice(args(json!({"document_id": DOCUMENT_ID})))
            .await
            .is_err()
    );
    assert_eq!(
        fake.requests_to("POST", &transform).await.len(),
        1,
        "a POST that may have run is not resent"
    );
}

#[tokio::test]
async fn an_update_reads_merges_and_writes_back_what_henrri_accepts() {
    let fake = Fake::start().await;
    let route = format!("/v1/customers/{CUSTOMER_ID}");
    fake.mount("PUT", &route, customer(CUSTOMER_ID, "NOUVEAU NOM"))
        .await;
    let server = server(&fake, &[]);
    server
        .update_customer(args(json!({
            "customer_id": CUSTOMER_ID,
            "name": "Nouveau nom",
            "address": {"city": "Colmar", "post_code": "68000"}
        })))
        .await
        .unwrap();
    let put = body(&fake.requests_to("PUT", &route).await[0]);
    assert_eq!(put["name"], "Nouveau nom");
    assert_eq!(put["comment"], "keep me", "unchanged fields are sent back");
    assert_eq!(put["isSupplier"], true);
    assert_eq!(put["address"]["city"], "Colmar");
    assert_eq!(
        put["address"]["address"], "1 rue de la Fibre",
        "nested objects merge"
    );
    assert!(put.get("contacts").is_none(), "contacts would be refused");
    assert!(
        put.get("vatNumber").is_none(),
        "an unchanged \"\" would be refused"
    );
}

#[tokio::test]
async fn an_empty_update_sends_nothing() {
    let fake = Fake::start().await;
    let server = server(&fake, &[]);
    let err = server
        .update_customer(args(json!({"customer_id": CUSTOMER_ID})))
        .await
        .err()
        .expect("the tool refuses");
    assert!(err.to_string().contains("nothing to update"), "{err}");
    assert!(
        fake.requests_to("PUT", &format!("/v1/customers/{CUSTOMER_ID}"))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn kinds_are_repeated_keys_and_periods_inclusive_dates() {
    let fake = Fake::start().await;
    fake.mount("GET", "/v1/documents", page(vec![])).await;
    let server = server(&fake, &[]);
    server
        .list_documents(args(json!({
            "document_types": ["invoice", "creditNote"],
            "state": "finalized",
            "period": "2026-02",
            "limit": 500
        })))
        .await
        .unwrap();
    let sent = &fake.requests_to("GET", "/v1/documents").await[0];
    let pairs: Vec<(String, String)> = sent
        .url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    for expected in [
        ("documentTypes", "invoice"),
        ("documentTypes", "creditNote"),
        ("state", "finalized"),
        ("fromDate", "2026-02-01"),
        ("toDate", "2026-02-28"),
        ("limit", "100"),
    ] {
        assert!(
            pairs.contains(&(expected.0.to_owned(), expected.1.to_owned())),
            "{expected:?} missing from {pairs:?}"
        );
    }
}

#[tokio::test]
async fn finalizing_needs_a_yes_and_sends_the_first_of_month_flag_as_query() {
    let fake = Fake::start().await;
    let finalize = format!("/v1/documents/{DOCUMENT_ID}/finalize");
    Mock::given(method("POST"))
        .and(path(finalize.as_str()))
        .and(query_param("confirmFirstOfMonth", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"finalized": true})))
        .mount(&fake.server)
        .await;
    let server = server(&fake, &[]);
    let refusal = server
        .finalize_document(args(json!({"document_id": DOCUMENT_ID})))
        .await
        .err()
        .expect("the tool refuses")
        .to_string();
    assert!(refusal.contains("confirm: true"), "{refusal}");
    assert!(
        refusal.contains("2880.00 €"),
        "the refusal states the total: {refusal}"
    );
    assert!(fake.requests_to("POST", &finalize).await.is_empty());

    server
        .finalize_document(args(json!({
            "document_id": DOCUMENT_ID, "confirm": true,
            "confirm_first_of_month": true, "date": "2026-10-31"
        })))
        .await
        .unwrap();
    let sent = &fake.requests_to("POST", &finalize).await[0];
    assert_eq!(body(sent), json!({"date": "2026-10-31T00:00:00"}));
}

#[tokio::test]
async fn a_line_kind_is_resolved_to_the_company_type_id() {
    let fake = Fake::start().await;
    let route = format!("/v1/documents/{DOCUMENT_ID}/lines");
    fake.mount("POST", &route, json!({"id": 1})).await;
    let server = server(&fake, &[]);
    server
        .add_document_line(args(json!({
            "document_id": DOCUMENT_ID, "kind": "text", "description": "Prestation à distance"
        })))
        .await
        .unwrap();
    assert_eq!(
        body(&fake.requests_to("POST", &route).await[0]),
        json!({"typeId": 6, "description": "Prestation à distance"})
    );
}

#[tokio::test]
async fn a_new_milestone_gets_its_due_date_in_a_second_call() {
    let fake = Fake::start().await;
    let schedule = format!("/v1/documents/{DOCUMENT_ID}/paymentmilestones");
    fake.mount(
        "GET",
        &schedule,
        json!({"elements": [{"id": 1, "amount": 100.0, "dueDate": "2026-11-09T00:00:00"}], "meta": {"totalCount": 1}}),
    )
    .await;
    fake.mount(
        "POST",
        "/v1/paymentmilestones",
        json!([
            {"id": 1, "amount": 60.0, "dueDate": "2026-11-09T00:00:00"},
            {"id": 2, "amount": 40.0, "percentage": 40.0, "dueDate": "2026-11-09T00:00:00", "isPaid": false, "paymentId": null}
        ]),
    )
    .await;
    fake.mount(
        "GET",
        "/v1/paymentmilestones/2",
        json!({"id": 2, "amount": 40.0, "percentage": 40.0, "dueDate": "2026-11-09T00:00:00", "isPaid": false, "paymentId": null, "documentId": DOCUMENT_ID}),
    )
    .await;
    fake.mount(
        "PUT",
        "/v1/paymentmilestones/2",
        json!([{"id": 1}, {"id": 2, "dueDate": "2026-12-24T00:00:00"}]),
    )
    .await;
    let server = server(&fake, &[]);
    let listing = server
        .create_payment_milestone(args(json!({
            "document_id": DOCUMENT_ID, "percentage": 40, "due_date": "2026-12-24"
        })))
        .await
        .unwrap();
    assert!(
        listing
            .0
            .note
            .as_deref()
            .unwrap_or("")
            .contains("second call")
    );
    let put = body(&fake.requests_to("PUT", "/v1/paymentmilestones/2").await[0]);
    assert_eq!(put["dueDate"], "2026-12-24T00:00:00");
    assert!(put.get("isPaid").is_none() && put.get("paymentId").is_none());
}

#[tokio::test]
async fn a_pdf_is_saved_under_a_safe_name_in_the_download_directory() {
    let fake = Fake::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/documents/{DOCUMENT_ID}/pdf")))
        .and(query_param("isFacturX", "true"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/pdf")
                .insert_header(
                    "content-disposition",
                    "attachment; filename=\"../Facture I-26-11-4.pdf\"",
                )
                .set_body_bytes(b"%PDF-1.7 fake".to_vec()),
        )
        .mount(&fake.server)
        .await;
    let dir = std::env::temp_dir().join(format!("mcp-henrri-test-{}", std::process::id()));
    let server = server(&fake, &[("HENRRI_DOWNLOAD_DIR", dir.to_str().unwrap())]);
    let saved = server
        .get_document_pdf(args(
            json!({"document_id": DOCUMENT_ID, "is_factur_x": true}),
        ))
        .await
        .unwrap()
        .0;
    assert_eq!(saved.file_name, "Facture I-26-11-4.pdf");
    assert_eq!(saved.bytes, 13);
    assert_eq!(std::fs::read(&saved.path).unwrap(), b"%PDF-1.7 fake");
    assert!(std::path::Path::new(&saved.path).starts_with(&dir));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_api_refusal_is_a_tool_result_with_the_problem_document() {
    let fake = Fake::start().await;
    fake.fail(
        "DELETE",
        &format!("/v1/documents/{DOCUMENT_ID}"),
        400,
        json!({
            "type": "https://api.henrri.io/scalar/#tag/businesserrors",
            "title": "Accounting document", "status": 400,
            "detail": "Finalized accounting documents cannot be deleted.",
            "errorCode": "ACCOUNTING_DOCUMENT", "traceId": "00-trace-01"
        }),
    )
    .await;
    let server = server(&fake, &[]);
    let err = server
        .delete_document(args(json!({"document_id": DOCUMENT_ID, "confirm": true})))
        .await
        .err()
        .expect("the tool refuses");
    assert!(matches!(&err.0, Error::Api { .. }));
    let rmcp::model::CallToolResponse::Complete(result) = Err::<rmcp::Json<Value>, _>(err)
        .into_call_tool_result()
        .unwrap()
    else {
        panic!("a complete result");
    };
    assert_eq!(result.is_error, Some(true));
    let text = serde_json::to_string(&result.content).unwrap();
    assert!(
        text.contains("ACCOUNTING_DOCUMENT") && text.contains("00-trace-01"),
        "{text}"
    );
}

#[tokio::test]
async fn credit_headers_are_kept_for_get_api_usage() {
    let fake = Fake::bare().await;
    Mock::given(method("GET"))
        .and(path("/v1/secures/hello-world"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-credits-used", "42")
                .insert_header("x-credits-limit", "200")
                .insert_header("x-credits-remaining", "158")
                .set_body_json(json!("Hello World")),
        )
        .mount(&fake.server)
        .await;
    let server = server(&fake, &[]);
    let usage = server.get_api_usage().await.unwrap().0;
    assert_eq!(usage.usage.credits_used.as_deref(), Some("42"));
    assert_eq!(usage.usage.credits_remaining.as_deref(), Some("158"));
}

#[tokio::test]
async fn payments_search_with_the_parameter_that_route_spells() {
    let fake = Fake::start().await;
    fake.mount("GET", "/v1/payments", page(vec![])).await;
    let server = server(&fake, &[]);
    server
        .list_payments(args(json!({"search": "Martin"})))
        .await
        .unwrap();
    let sent = &fake.requests_to("GET", "/v1/payments").await[0];
    let query = sent.url.query().unwrap_or_default();
    assert!(query.contains("SearchString=Martin"), "{query}");
    assert!(!query.contains("search="), "{query}");
}

#[tokio::test]
async fn a_unit_answered_with_id_zero_is_found_by_its_name() {
    let fake = Fake::start().await;
    fake.mount(
        "POST",
        "/v1/units",
        json!({"id": 0, "name": "jour", "unitKind": null}),
    )
    .await;
    fake.mount(
        "GET",
        "/v1/units",
        page(vec![
            json!({"id": 77, "name": "jour", "unitKind": "custom"}),
        ]),
    )
    .await;
    let server = server(&fake, &[]);
    let unit = server
        .create_unit(args(json!({"name": "jour"})))
        .await
        .unwrap();
    assert_eq!(unit.0.0["id"], 77);
}
