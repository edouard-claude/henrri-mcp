//! MCP resources: the same data as the read tools, for clients that would
//! rather attach context than call a function.
//!
//! Five fixed URIs and four templates, all read-only. Everything is JSON except
//! a document's PDF, which is served as a base64 blob.

use base64::Engine as _;
use rmcp::model::{ReadResourceResult, Resource, ResourceContents, ResourceTemplate};
use serde_json::{Value, json};

use crate::{
    client::Request,
    error::{Error, Result},
    server::HenrriServer,
    tools::{Id, Listing},
    webhooks,
};

/// The URI scheme every resource of this server lives under.
pub const SCHEME: &str = "henrri://";

/// The authenticated user and their company.
pub const URI_ME: &str = "henrri://me";
/// Every reference list tools take ids from.
pub const URI_REFERENCE: &str = "henrri://reference";
/// The credit and rate-limit counters.
pub const URI_USAGE: &str = "henrri://usage";
/// The webhook event catalogue.
pub const URI_WEBHOOK_EVENTS: &str = "henrri://webhooks/events";
/// The webhook deliveries received.
pub const URI_WEBHOOK_RECEIVED: &str = "henrri://webhooks/received";

fn json_resource(uri: &str, name: &str, title: &str, description: &str) -> Resource {
    Resource::new(uri, name)
        .with_title(title)
        .with_description(description)
        .with_mime_type("application/json")
}

/// The resources a client can read without filling in anything.
pub fn catalogue() -> Vec<Resource> {
    vec![
        json_resource(
            URI_ME,
            "me",
            "Me and my company",
            "The authenticated Henrri user and their company.",
        ),
        json_resource(
            URI_REFERENCE,
            "reference",
            "Reference lists",
            "Document types, line types, payment modes, item categories and units, with the ids the tools take.",
        ),
        json_resource(
            URI_USAGE,
            "usage",
            "API credit usage",
            "Credits used and left this month, and the rate limit, from the last answer's headers.",
        ),
        json_resource(
            URI_WEBHOOK_EVENTS,
            "webhook-events",
            "Webhook events",
            "Every event Henrri can send, and how deliveries are signed.",
        ),
        json_resource(
            URI_WEBHOOK_RECEIVED,
            "webhook-received",
            "Received webhook events",
            "The verified webhook deliveries this server received over HTTP, newest first.",
        ),
    ]
}

/// The resources a client reads by filling in a variable.
pub fn templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate::new("henrri://documents/{document_id}", "document")
            .with_title("A document")
            .with_description(
                "One document with everything: lines, customer, company, tax details. Amounts in euros.",
            )
            .with_mime_type("application/json"),
        ResourceTemplate::new("henrri://documents/{document_id}/pdf", "document-pdf")
            .with_title("A document's PDF")
            .with_description("The document rendered as a PDF, as a base64 blob.")
            .with_mime_type("application/pdf"),
        ResourceTemplate::new("henrri://customers/{customer_id}", "customer")
            .with_title("A customer")
            .with_description("One customer with its contacts and its payment milestones.")
            .with_mime_type("application/json"),
        ResourceTemplate::new("henrri://revenues/{year}", "revenue")
            .with_title("A year's revenue")
            .with_description("A year's revenue, in total and month by month, in euros.")
            .with_mime_type("application/json"),
    ]
}

fn parse_id(raw: &str) -> Result<Id> {
    raw.parse::<u32>()
        .ok()
        .and_then(Id::new)
        .ok_or_else(|| Error::invalid(format!("{raw:?} is not an id")))
}

/// Read one resource by URI.
pub async fn read(server: &HenrriServer, uri: &str) -> Result<ReadResourceResult> {
    let client = server.client();
    let get = |path: String| async move { client.call::<Value>(Request::get(path)).await };
    let list = |path: &'static str| async move {
        client
            .call::<Value>(Request::get(path))
            .await
            .map(|v| Listing::from_value(v).items)
    };
    let json: Value = match uri {
        URI_ME => get("/v1/users/me".into()).await?,
        URI_REFERENCE => json!({
            "documentTypes": list("/v1/documenttypes").await?,
            "documentLineTypes": list("/v1/documentlinetypes").await?,
            "paymentModes": list("/v1/paymentmodes").await?,
            "itemCategories": list("/v1/itemcategories").await?,
            "units": list("/v1/units").await?,
        }),
        URI_USAGE => {
            if client.usage().credits_used.is_none() {
                let _ = get("/v1/secures/hello-world".into()).await?;
            }
            to_json(uri, &client.usage())?
        }
        URI_WEBHOOK_EVENTS => to_json(uri, &webhooks::EVENTS)?,
        URI_WEBHOOK_RECEIVED => {
            to_json(uri, &server.inbox().recent(None, webhooks::INBOX_CAPACITY))?
        }
        other => {
            let rest = other.strip_prefix(SCHEME).ok_or_else(|| unknown(other))?;
            let parts: Vec<&str> = rest.split('/').collect();
            match parts.as_slice() {
                ["documents", id] => {
                    let id = parse_id(id)?;
                    get(format!("/v1/documents/{id}/with-all")).await?
                }
                ["documents", id, "pdf"] => {
                    let id = parse_id(id)?;
                    let response = client
                        .send(
                            Request::get(format!("/v1/documents/{id}/pdf"))
                                .binary("application/pdf"),
                        )
                        .await?;
                    let blob = base64::engine::general_purpose::STANDARD.encode(&response.bytes);
                    return Ok(ReadResourceResult::new(vec![
                        ResourceContents::blob(blob, uri).with_mime_type("application/pdf"),
                    ]));
                }
                ["customers", id] => {
                    let id = parse_id(id)?;
                    let mut customer = get(format!("/v1/customers/{id}")).await?;
                    let milestones = get(format!("/v1/customers/{id}/paymentmilestones")).await?;
                    customer["paymentMilestones"] =
                        Value::Array(Listing::from_value(milestones).items);
                    customer
                }
                ["revenues", year] => {
                    let year: i32 = year
                        .parse()
                        .ok()
                        .filter(|y| (1000..=9999).contains(y))
                        .ok_or_else(|| Error::invalid(format!("{year:?} is not a year")))?;
                    json!({
                        "year": get(format!("/v1/revenues/{year}")).await?,
                        "months": get(format!("/v1/revenues/{year}/months")).await?,
                    })
                }
                _ => return Err(unknown(other)),
            }
        }
    };

    let text = serde_json::to_string_pretty(&json).map_err(|source| Error::Decode {
        endpoint: uri.to_owned(),
        source,
    })?;
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(text, uri).with_mime_type("application/json"),
    ]))
}

fn to_json<T: serde::Serialize>(uri: &str, value: &T) -> Result<Value> {
    serde_json::to_value(value).map_err(|source| Error::Decode {
        endpoint: uri.to_owned(),
        source,
    })
}

fn unknown(uri: &str) -> Error {
    Error::invalid(format!(
        "no resource at {uri:?}. This server serves {URI_ME}, {URI_REFERENCE}, {URI_USAGE}, {URI_WEBHOOK_EVENTS}, {URI_WEBHOOK_RECEIVED}, henrri://documents/{{document_id}}, henrri://documents/{{document_id}}/pdf, henrri://customers/{{customer_id}} and henrri://revenues/{{year}}."
    ))
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

    #[test]
    fn the_catalogue_and_the_templates_are_documented() {
        for resource in catalogue() {
            assert!(resource.uri.starts_with(SCHEME), "{}", resource.uri);
            assert!(resource.title.is_some(), "{} has no title", resource.uri);
            assert!(resource.description.is_some(), "{}", resource.uri);
        }
        for template in templates() {
            assert!(template.uri_template.starts_with(SCHEME));
            assert!(template.uri_template.contains('{'), "no variable");
            assert!(template.description.is_some());
        }
    }

    #[test]
    fn an_unknown_uri_lists_what_exists() {
        let message = unknown("henrri://nope").to_string();
        assert!(message.contains(URI_REFERENCE), "{message}");
        assert!(message.contains("documents/{document_id}/pdf"), "{message}");
    }
}
