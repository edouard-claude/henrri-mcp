//! A fake Henrri, so the tests exercise the real client and the real MCP
//! surface without touching anybody's invoices.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    dead_code,
    reason = "test support code"
)]

use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

/// The company every test works on.
pub const COMPANY_ID: i64 = 947;
/// A customer the fake already holds.
pub const CUSTOMER_ID: i64 = 10_363;
/// A draft invoice the fake already holds.
pub const DOCUMENT_ID: i64 = 9_632;
/// The webhook secret the tests sign with.
pub const WEBHOOK_SECRET: &str = "whsec-test";

/// A running fake Henrri.
pub struct Fake {
    pub server: MockServer,
}

impl Fake {
    /// Start a fake with only the token endpoint mounted.
    pub async fn bare() -> Self {
        let fake = Self {
            server: MockServer::start().await,
        };
        Mock::given(method("POST"))
            .and(path("/v1/users/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "jwt-test",
                "expires_in": 600,
                "token_type": "Bearer",
                "refresh_token": "refresh-test",
                "scope": "Document_Read Document_Write"
            })))
            .mount(&fake.server)
            .await;
        fake
    }

    /// Start a fake with the read surface mounted.
    pub async fn start() -> Self {
        let fake = Self::bare().await;
        fake.mount("GET", "/v1/secures/hello-world", json!("Hello World"))
            .await;
        fake.mount(
            "GET",
            "/v1/users/me",
            json!({
                "user": { "id": 16079, "email": "me@example.com", "role": "admin" },
                "company": { "id": COMPANY_ID, "name": "Atelier Test", "subscriptionPlan": "discovery" }
            }),
        )
        .await;
        fake.mount(
            "GET",
            "/v1/customers",
            page(vec![customer(CUSTOMER_ID, "MON CLIENT PRO")]),
        )
        .await;
        fake.mount(
            "GET",
            &format!("/v1/customers/{CUSTOMER_ID}"),
            customer(CUSTOMER_ID, "MON CLIENT PRO"),
        )
        .await;
        fake.mount(
            "GET",
            &format!("/v1/documents/{DOCUMENT_ID}"),
            json!({
                "id": DOCUMENT_ID, "identity": "I-26-11-4", "finalized": false, "type": "Facture",
                "documentTypeId": 1, "customerId": CUSTOMER_ID, "title": "Mission", "subtitle": null,
                "priceAfterTax": 2880.0, "customer": { "id": CUSTOMER_ID, "name": "MON CLIENT PRO" },
                "documentType": { "id": 1, "documentKind": "invoice", "isAccounting": true },
                "lines": null
            }),
        )
        .await;
        fake.mount(
            "GET",
            "/v1/documentlinetypes",
            json!({
                "elements": [
                    { "id": 3, "label": "Article", "type": "item" },
                    { "id": 6, "label": "Texte", "type": "text" },
                    { "id": 5, "label": "S/Total", "type": "subtotal" }
                ],
                "meta": { "totalCount": 3 }
            }),
        )
        .await;
        fake
    }

    /// The API root the server under test should use.
    pub fn base_url(&self) -> String {
        self.server.uri()
    }

    /// Answer `method path` with a JSON body.
    pub async fn mount(&self, verb: &str, route: &str, body: Value) {
        Mock::given(method(verb))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.server)
            .await;
    }

    /// Answer `method path` with a status and a problem document.
    pub async fn fail(&self, verb: &str, route: &str, status: u16, problem: Value) {
        Mock::given(method(verb))
            .and(path(route))
            .respond_with(
                ResponseTemplate::new(status)
                    .insert_header("content-type", "application/problem+json")
                    .set_body_json(problem),
            )
            .mount(&self.server)
            .await;
    }

    /// Every request the fake received, as `METHOD /path?query`.
    pub async fn requests(&self) -> Vec<wiremock::Request> {
        self.server.received_requests().await.unwrap_or_default()
    }

    /// The requests to one route, token requests excluded.
    pub async fn requests_to(&self, verb: &str, route: &str) -> Vec<wiremock::Request> {
        self.requests()
            .await
            .into_iter()
            .filter(|r| r.method.as_str() == verb && r.url.path() == route)
            .collect()
    }
}

/// Henrri's paging envelope.
pub fn page(elements: Vec<Value>) -> Value {
    let count = elements.len();
    json!({
        "elements": Value::Array(elements),
        "totals": null,
        "display": null,
        "meta": { "page": 1, "limit": 50, "totalCount": count, "totalPages": 1, "hasNext": false }
    })
}

/// A customer as Henrri answers it.
pub fn customer(id: i64, name: &str) -> Value {
    json!({
        "id": id, "parentCustomerId": null, "name": name, "type": "professional",
        "siret": "", "vatNumber": "", "tradeName": name, "comment": "keep me",
        "website": "https://example.com", "isSupplier": true,
        "address": { "id": 1, "address": "1 rue de la Fibre", "city": "Mulhouse", "postCode": "68100", "country": "France" },
        "contacts": [{ "id": 5, "lastName": "Martin", "isPrimary": true }]
    })
}

/// The environment the binary or the config is built from.
pub fn env(fake: &Fake) -> Vec<(String, String)> {
    vec![
        ("HENRRI_USERNAME".into(), "me@example.com".into()),
        ("HENRRI_PASSWORD".into(), "pw".into()),
        ("HENRRI_BASE_URL".into(), fake.base_url()),
        ("HENRRI_MAX_RETRIES".into(), "2".into()),
        ("HENRRI_WEBHOOK_SECRET".into(), WEBHOOK_SECRET.into()),
    ]
}

/// A configuration pointing at the fake.
pub fn config(fake: &Fake, extra: &[(&str, &str)]) -> mcp_henrri::Config {
    let mut vars = env(fake);
    vars.extend(
        extra
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
    );
    mcp_henrri::Config::from_getter(move |key| {
        vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    })
    .expect("test configuration")
}
