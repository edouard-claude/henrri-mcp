//! The MCP tools, one module per toolset.
//!
//! Every operation of the Henrri v1 specification is reachable: 85 routes, one
//! tool each, plus the two authentication routes the client drives itself
//! (see [`crate::coverage`]). Four tools have no route of their own:
//! `get_api_usage`, because the credit counters only travel in headers, and
//! three webhook tools, because webhooks are pushed to you rather than pulled.
//!
//! Naming follows the same rule everywhere: `verb_noun`, and the doc comment of
//! the function is the description a model reads.

pub mod account;
pub mod catalog;
pub mod customers;
pub mod documents;
pub mod lines;
pub mod payments;
pub mod revenues;
pub mod webhooks;

use std::{borrow::Cow, fmt, num::NonZeroU32};

use rmcp::handler::server::router::tool::ToolRouter;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::{
    client::{Query, Request},
    config::{Config, Toolset},
    dates,
    error::{Error, ToolError},
    server::HenrriServer,
};

/// Assemble the tool router the configuration asks for.
///
/// Read-only mode is not a list of tool names to keep in sync: it drops every
/// route that does not annotate itself `readOnlyHint`, so a new write tool is
/// excluded by construction.
pub fn router(config: &Config) -> ToolRouter<HenrriServer> {
    let mut router = ToolRouter::new();
    if config.has(Toolset::Account) {
        router += HenrriServer::account_router();
    }
    if config.has(Toolset::Customers) {
        router += HenrriServer::customers_router();
    }
    if config.has(Toolset::Documents) {
        router += HenrriServer::documents_router();
        router += HenrriServer::lines_router();
    }
    if config.has(Toolset::Catalog) {
        router += HenrriServer::catalog_router();
    }
    if config.has(Toolset::Payments) {
        router += HenrriServer::payments_router();
    }
    if config.has(Toolset::Revenues) {
        router += HenrriServer::revenues_router();
    }
    if config.has(Toolset::Webhooks) {
        router += HenrriServer::webhooks_router();
    }

    if config.read_only() {
        let writers: Vec<String> = router
            .list_all()
            .into_iter()
            .filter(|tool| {
                !tool
                    .annotations
                    .as_ref()
                    .and_then(|a| a.read_only_hint)
                    .unwrap_or(false)
            })
            .map(|tool| tool.name.to_string())
            .collect();
        for name in writers {
            router.remove_route(&name);
        }
    }

    router
}

/// A Henrri identifier: a positive 32-bit integer.
///
/// Models sometimes quote numbers, so `"42"` is read as `42`; zero and
/// negative values are refused before any request leaves the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(NonZeroU32);

impl Id {
    /// The identifier as the API spells it.
    pub fn get(self) -> u32 {
        self.0.get()
    }

    /// Build an identifier, refusing zero.
    pub fn new(value: u32) -> Option<Self> {
        NonZeroU32::new(value).map(Self)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for Id {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.get())
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Number(i64),
            Text(String),
        }
        let number = match Raw::deserialize(deserializer)? {
            Raw::Number(n) => n,
            Raw::Text(s) => s.trim().parse::<i64>().map_err(|_| {
                serde::de::Error::custom(format!("{s:?} is not an identifier (a positive integer)"))
            })?,
        };
        // Henrri ids are int32: anything above i32::MAX names nothing.
        i32::try_from(number)
            .ok()
            .and_then(|n| u32::try_from(n).ok())
            .and_then(Self::new)
            .ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "{number} is not an identifier: Henrri ids are positive 32-bit integers"
                ))
            })
    }
}

impl schemars::JsonSchema for Id {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Id")
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "integer", "minimum": 1, "maximum": 2_147_483_647 })
    }
}

/// One Henrri object, as the API returned it.
///
/// Henrri's models are large and grow every month, so they are passed through
/// whole rather than squeezed into structs that would silently drop what they
/// do not know. Amounts are in euros, as decimals.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct ApiObject(pub Map<String, Value>);

impl ApiObject {
    /// Wrap whatever came back; a non-object is kept under `value`.
    pub fn from_value(value: Value) -> Self {
        match value {
            Value::Object(map) => Self(map),
            Value::Null => Self(Map::new()),
            other => {
                let mut map = Map::new();
                map.insert("value".into(), other);
                Self(map)
            }
        }
    }
}

/// What a list tool answers with.
#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
pub struct Listing {
    /// How many objects this answer carries.
    pub returned: usize,
    /// The page this answer is, from 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<i64>,
    /// The page size used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    /// How many objects match, all pages together.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<i64>,
    /// How many pages there are.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pages: Option<i64>,
    /// True when another page follows: call again with `page` + 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_next: Option<bool>,
    /// The objects. Amounts are in euros.
    pub items: Vec<Value>,
    /// The totals row, on the table listings that were asked for one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub totals: Option<Value>,
    /// Set when the answer is worth a word of explanation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Listing {
    /// Read Henrri's `{ elements, meta }` envelope, or a bare array.
    pub fn from_value(value: Value) -> Self {
        match value {
            Value::Array(items) => Self::all(items),
            Value::Null => Self::all(Vec::new()),
            Value::Object(mut map) => {
                let items = match map.remove("elements") {
                    Some(Value::Array(items)) => items,
                    _ => match map.remove("taxDetailArray") {
                        Some(Value::Array(items)) => items,
                        _ => return Self::all(vec![Value::Object(map)]),
                    },
                };
                let meta = map.remove("meta").unwrap_or(Value::Null);
                let int = |key: &str| meta.get(key).and_then(Value::as_i64);
                // An empty answer says page 0 of 0; that is not a page number.
                let positive = |key: &str| int(key).filter(|n| *n > 0);
                Self {
                    returned: items.len(),
                    page: positive("page"),
                    limit: positive("limit"),
                    total_count: int("totalCount"),
                    total_pages: positive("totalPages"),
                    has_next: meta.get("hasNext").and_then(Value::as_bool),
                    items,
                    totals: map.remove("totals").filter(|t| !t.is_null()),
                    note: None,
                }
            }
            other => Self::all(vec![other]),
        }
    }

    /// Read a `with-selected-fields` table: each row's cells become one object
    /// keyed by column, so the model reads `{"MontantHT": "5 300,00"}` rather
    /// than positional cells.
    pub fn from_table(value: Value) -> Self {
        let columns: Vec<String> = value
            .get("display")
            .and_then(Value::as_array)
            .map(|cols| {
                let mut cols: Vec<&Value> = cols.iter().collect();
                cols.sort_by_key(|c| c.get("index").and_then(Value::as_i64).unwrap_or(0));
                cols.iter()
                    .map(|c| {
                        c.get("title")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut listing = Self::from_value(value);
        listing.items = listing
            .items
            .into_iter()
            .map(|row| row_to_object(&row, &columns))
            .collect();
        listing.totals = listing.totals.map(|cells| {
            let cells = serde_json::json!({ "cells": cells });
            row_to_object(&cells, &columns)
        });
        listing
    }

    /// Wrap objects that were not paginated upstream.
    pub fn all(items: Vec<Value>) -> Self {
        Self {
            returned: items.len(),
            total_count: i64::try_from(items.len()).ok(),
            items,
            ..Self::default()
        }
    }

    /// Add the word of explanation.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// One table row, `{ cells: [{ name, value, … }] }`, as `{ name: value }`.
fn row_to_object(row: &Value, columns: &[String]) -> Value {
    let Some(cells) = row.get("cells").and_then(Value::as_array) else {
        return row.clone();
    };
    let mut object = Map::new();
    for (index, cell) in cells.iter().enumerate() {
        let name = cell
            .get("name")
            .and_then(Value::as_str)
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
            .or_else(|| columns.get(index).filter(|c| !c.is_empty()).cloned())
            .unwrap_or_else(|| format!("column_{}", index + 1));
        object.insert(name, cell.get("value").cloned().unwrap_or(Value::Null));
    }
    Value::Object(object)
}

/// What a write tool with an empty upstream answer reports.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Done {
    /// Always true: a failure is an error result, not a `false`.
    pub ok: bool,
    /// What was done, in one sentence.
    pub action: String,
    /// What Henrri answered, when it answered something.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
}

impl Done {
    /// Report a successful write.
    pub fn new(action: impl Into<String>, response: Value) -> Self {
        Self {
            ok: true,
            action: action.into(),
            response: (!response.is_null()).then_some(response),
        }
    }
}

/// Sort direction, as Henrri spells it.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    /// Smallest or oldest first, the API's default.
    Ascending,
    /// Largest or newest first.
    Descending,
}

/// Paging and searching, the same on every list tool.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct PageArgs {
    /// The page to read, from 1. Default 1.
    pub page: Option<u32>,
    /// Objects per page, 1 to 100. Default 50.
    pub limit: Option<u32>,
    /// Free text, matched against the columns Henrri shows in its own list.
    pub search: Option<String>,
    /// The field to sort on. A wrong value is refused with the list of the
    /// allowed ones.
    pub sort_by: Option<String>,
    /// `ascending` (default) or `descending`.
    pub sort_order: Option<SortOrder>,
}

/// A creation-date range, inclusive on both ends.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct DateArgs {
    /// A period: `this month`, `last month`, `2026-09`, `2026`, `last 30 days`,
    /// `today`… Explicit `from_date` / `to_date` win over it.
    pub period: Option<String>,
    /// First day kept, inclusive: `YYYY-MM-DD`, `DD/MM/YYYY`, `today`…
    pub from_date: Option<String>,
    /// Last day kept, inclusive.
    pub to_date: Option<String>,
}

/// The two extra switches of the `with-selected-fields` table listings.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct TableArgs {
    /// Add a totals row for the numeric columns.
    pub with_totals: Option<bool>,
    /// Compute the totals on the current page only, rather than every match.
    pub only_current_page: Option<bool>,
}

/// The page size Henrri refuses to exceed.
pub const MAX_PAGE_SIZE: u32 = 100;

/// A query string under construction. Absent arguments stay absent.
#[derive(Debug, Default)]
pub struct QueryBuilder(Query);

impl QueryBuilder {
    /// An empty query.
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Add a pair when there is a value.
    #[must_use]
    pub fn opt(mut self, key: &str, value: Option<impl ToString>) -> Self {
        if let Some(value) = value {
            let value = value.to_string();
            if !value.trim().is_empty() {
                self.0.push((key.to_owned(), value));
            }
        }
        self
    }

    /// Add an enum by its API spelling.
    #[must_use]
    pub fn opt_enum<T: Serialize>(self, key: &str, value: Option<T>) -> Self {
        let value = value.map(|v| api_name(&v));
        self.opt(key, value)
    }

    /// Add a list as repeated keys. Henrri reads a comma-separated value as one
    /// single value and, contrary to its documentation, answers an empty page
    /// rather than an error.
    #[must_use]
    pub fn list<T: Serialize>(mut self, key: &str, values: Option<&[T]>) -> Self {
        for value in values.unwrap_or_default() {
            self.0.push((key.to_owned(), api_name(value)));
        }
        self
    }

    /// Fold paging, searching and sorting in.
    #[must_use]
    pub fn paged(self, page: &PageArgs) -> Self {
        self.opt("page", page.page.map(|p| p.max(1)))
            .opt("limit", page.limit.map(|l| l.clamp(1, MAX_PAGE_SIZE)))
            .opt("search", page.search.as_deref().map(str::trim))
            .opt("sortBy", page.sort_by.as_deref().map(str::trim))
            .opt_enum("sortOrder", page.sort_order)
    }

    /// Fold an inclusive date range in.
    pub fn dated(self, dates: &DateArgs) -> crate::error::Result<Self> {
        let (from, to) = dates::range(
            dates.period.as_deref(),
            dates.from_date.as_deref(),
            dates.to_date.as_deref(),
        )?;
        Ok(self.opt("fromDate", from).opt("toDate", to))
    }

    /// Fold the table switches in.
    #[must_use]
    pub fn table(self, table: &TableArgs) -> Self {
        self.opt("withTotals", table.with_totals)
            .opt("onlyCurrentPage", table.only_current_page)
    }

    /// The query, ready to send.
    pub fn build(self) -> Query {
        self.0
    }
}

/// An enum's spelling on the wire, which is its serde name.
pub fn api_name<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

/// Turn tool arguments into an API body: snake_case keys become camelCase,
/// absent (`null`) fields are dropped, recursively.
///
/// Tool arguments are snake_case, as MCP tools conventionally are; Henrri's
/// bodies are camelCase. The two spellings map one to one
/// (`days_number_before_payment_reminder_level1` →
/// `daysNumberBeforePaymentReminderLevel1`), so one function does it for every
/// body instead of a rename attribute on every field.
pub fn to_api<T: Serialize>(args: &T) -> crate::error::Result<Value> {
    let value = serde_json::to_value(args).map_err(|source| Error::Decode {
        endpoint: "tool arguments".into(),
        source,
    })?;
    Ok(camelize(value))
}

fn camelize(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (camel_case(&k), camelize(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(camelize).collect()),
        other => other,
    }
}

/// `is_tax_included` → `isTaxIncluded`.
pub fn camel_case(snake: &str) -> String {
    let mut out = String::with_capacity(snake.len());
    let mut upper = false;
    for c in snake.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Lay `patch` over `base`: nested objects merge, everything else replaces.
pub fn overlay(base: &mut Map<String, Value>, patch: Map<String, Value>) {
    for (key, value) in patch {
        match (base.get_mut(&key), value) {
            (Some(Value::Object(inner)), Value::Object(patch)) => overlay(inner, patch),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

/// Refuse an irreversible call that was not confirmed.
///
/// Finalizing, validating, paying and deleting cannot be undone through the
/// API, and finalizing costs credits, so these are fail-closed: without
/// `confirm: true` nothing leaves the process.
pub fn require_confirmation(confirm: Option<bool>, what: &str) -> Result<(), ToolError> {
    if confirm == Some(true) {
        return Ok(());
    }
    Err(Error::invalid(format!(
        "{what}. This cannot be undone through the API. Show the person exactly what is about to happen, get their explicit agreement, then call again with confirm: true."
    ))
    .into())
}

impl HenrriServer {
    /// Read, merge and write back: Henrri's `PUT` replaces the whole resource,
    /// and a field left out is erased (a customer `PUT` without `comment`
    /// deletes the comment). Update tools take only what changes; this reads
    /// the current state, lays the change over it, drops what `adjust` says
    /// the API refuses to receive back, and sends the whole.
    pub async fn modify(
        &self,
        path: &str,
        patch: Value,
        adjust: impl FnOnce(&mut Map<String, Value>, &Map<String, Value>),
    ) -> crate::error::Result<Value> {
        let Value::Object(patch) = patch else {
            return Err(Error::invalid("an update takes an object of fields"));
        };
        if patch.is_empty() {
            return Err(Error::invalid(
                "nothing to update: pass at least one field to change",
            ));
        }
        let current: Value = self.client().call(Request::get(path)).await?;
        let Value::Object(mut merged) = current else {
            return Err(Error::invalid(format!("{path} did not answer an object")));
        };
        merged.remove("links");
        let changes = patch.clone();
        overlay(&mut merged, patch);
        // Henrri reads some unset text fields back as "" (vatNumber, siret…)
        // and refuses "" on write (minimum length 2): an unchanged empty
        // string is left out, which the PUT reads as unset, the same thing.
        merged.retain(|key, value| value.as_str() != Some("") || changes.contains_key(key));
        adjust(&mut merged, &changes);
        self.client()
            .call(Request::put(path).json(Value::Object(merged)))
            .await
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
    use serde_json::json;

    use super::*;
    use crate::config::Config;

    fn config(pairs: &[(&str, &str)]) -> Config {
        let mut owned: Vec<(String, String)> = vec![
            ("HENRRI_USERNAME".into(), "me@example.com".into()),
            ("HENRRI_PASSWORD".into(), "pw".into()),
        ];
        owned.extend(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        Config::from_getter(|key| owned.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()))
            .expect("test configuration")
    }

    fn tool_names(router: &ToolRouter<HenrriServer>) -> Vec<String> {
        router
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect()
    }

    #[test]
    fn every_operation_of_the_spec_has_a_tool() {
        let router = router(&config(&[]));
        // 85 routes, plus get_api_usage and the three webhook tools.
        assert_eq!(router.list_all().len(), 89, "{:?}", tool_names(&router));
    }

    #[test]
    fn the_coverage_table_names_exactly_the_registered_tools() {
        use crate::coverage::{EXTRA_TOOLS, OPERATIONS, Via};
        let mut expected: Vec<String> = OPERATIONS
            .iter()
            .filter_map(|(_, _, _, via)| match via {
                Via::Tool(name) => Some((*name).to_owned()),
                Via::Client => None,
            })
            .chain(EXTRA_TOOLS.iter().map(|(name, _)| (*name).to_owned()))
            .collect();
        expected.sort();
        let mut registered = tool_names(&router(&config(&[])));
        registered.sort();
        assert_eq!(registered, expected);
    }

    #[test]
    fn a_toolset_selection_only_exposes_that_toolset() {
        let names = tool_names(&router(&config(&[("HENRRI_TOOLSETS", "customers")])));
        assert!(names.contains(&"list_customers".to_owned()));
        assert!(!names.contains(&"finalize_document".to_owned()));
        assert!(!names.contains(&"pay_payment_milestone".to_owned()));
    }

    #[test]
    fn read_only_mode_keeps_only_tools_that_declare_themselves_read_only() {
        let router = router(&config(&[("HENRRI_READ_ONLY", "1")]));
        for tool in router.list_all() {
            assert_eq!(
                tool.annotations.as_ref().and_then(|a| a.read_only_hint),
                Some(true),
                "{} survived read-only mode",
                tool.name
            );
        }
        let names = tool_names(&router);
        assert!(names.contains(&"list_documents".to_owned()));
        for writer in [
            "finalize_document",
            "delete_customer",
            "create_item",
            "pay_payment_milestones",
        ] {
            assert!(!names.contains(&writer.to_owned()), "{writer} survived");
        }
    }

    #[test]
    fn every_tool_is_named_annotated_and_described() {
        let router = router(&config(&[]));
        for tool in router.list_all() {
            let annotations = tool
                .annotations
                .as_ref()
                .unwrap_or_else(|| panic!("{} has no annotations", tool.name));
            assert!(annotations.title.is_some(), "{} has no title", tool.name);
            assert!(
                annotations.read_only_hint.is_some(),
                "{} does not say whether it writes",
                tool.name
            );
            assert!(
                annotations.open_world_hint.is_some(),
                "{} does not say whether it leaves the process",
                tool.name
            );
            let description = tool
                .description
                .as_deref()
                .unwrap_or_else(|| panic!("{} has no description", tool.name));
            assert!(
                description.len() > 40,
                "{} has a description too short to be useful: {description:?}",
                tool.name
            );
            assert!(
                tool.name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_'),
                "{} is not snake_case",
                tool.name
            );
            assert!(
                tool.output_schema.is_some(),
                "{} has no output schema",
                tool.name
            );
        }
    }

    #[test]
    fn irreversible_tools_are_marked_destructive() {
        let router = router(&config(&[]));
        let destructive: Vec<String> = router
            .list_all()
            .into_iter()
            .filter(|t| t.annotations.as_ref().and_then(|a| a.destructive_hint) == Some(true))
            .map(|t| t.name.to_string())
            .collect();
        for name in [
            "delete_customer",
            "delete_customer_contact",
            "delete_document",
            "delete_document_line",
            "delete_item",
            "delete_text_item",
            "delete_payment_milestone",
            "cancel_deposit",
            "finalize_document",
        ] {
            assert!(
                destructive.contains(&name.to_owned()),
                "{name} is not destructive"
            );
        }
    }

    #[test]
    fn an_unconfirmed_call_says_what_to_do() {
        let err = require_confirmation(None, "invoice 12 would be finalized").unwrap_err();
        assert!(err.to_string().contains("confirm: true"), "{err}");
        assert!(require_confirmation(Some(false), "x").is_err());
        assert!(require_confirmation(Some(true), "x").is_ok());
    }

    #[test]
    fn ids_accept_numbers_and_quoted_numbers_but_not_zero() {
        let id: Id = serde_json::from_value(json!(42)).unwrap();
        assert_eq!(id.get(), 42);
        let id: Id = serde_json::from_value(json!("42")).unwrap();
        assert_eq!(id.get(), 42);
        assert!(serde_json::from_value::<Id>(json!(0)).is_err());
        assert!(serde_json::from_value::<Id>(json!(-3)).is_err());
        assert!(serde_json::from_value::<Id>(json!("abc")).is_err());
        assert!(serde_json::from_value::<Id>(json!(3_000_000_000_u64)).is_err());
    }

    #[test]
    fn the_envelope_is_read_and_an_empty_page_zero_dropped() {
        let listing = Listing::from_value(json!({
            "elements": [{"id": 1}, {"id": 2}],
            "meta": {"page": 1, "limit": 50, "totalCount": 137, "totalPages": 3, "hasNext": true}
        }));
        assert_eq!(listing.returned, 2);
        assert_eq!(listing.total_count, Some(137));
        assert_eq!(listing.has_next, Some(true));
        let empty = Listing::from_value(json!({
            "elements": [], "totals": null,
            "meta": {"page": 0, "limit": 50, "totalCount": 0, "totalPages": 0, "hasNext": false}
        }));
        assert_eq!(empty.page, None);
        assert_eq!(empty.total_count, Some(0));
    }

    #[test]
    fn table_rows_become_objects_keyed_by_column() {
        let listing = Listing::from_table(json!({
            "elements": [{"cells": [
                {"name": "TypeDocument", "value": "DEVIS"},
                {"name": null, "value": "5 300,00"}
            ]}],
            "display": [{"id": 1, "title": "Type", "index": 1}, {"id": 2, "title": "Montant HT", "index": 2}],
            "meta": {"page": 1, "limit": 50, "totalCount": 1, "totalPages": 1, "hasNext": false}
        }));
        assert_eq!(
            listing.items[0],
            json!({"TypeDocument": "DEVIS", "Montant HT": "5 300,00"})
        );
    }

    #[test]
    fn arguments_become_camel_case_without_nulls() {
        #[derive(Serialize)]
        struct Args {
            is_tax_included: Option<bool>,
            days_number_before_payment_reminder_level1: Option<i32>,
            address: Option<Value>,
            comment: Option<String>,
        }
        let body = to_api(&Args {
            is_tax_included: Some(true),
            days_number_before_payment_reminder_level1: Some(15),
            address: Some(json!({"post_code": "68100", "city": null})),
            comment: None,
        })
        .unwrap();
        assert_eq!(
            body,
            json!({
                "isTaxIncluded": true,
                "daysNumberBeforePaymentReminderLevel1": 15,
                "address": {"postCode": "68100"}
            })
        );
    }

    #[test]
    fn an_overlay_merges_nested_objects_and_replaces_the_rest() {
        let mut base =
            json!({"name": "A", "address": {"city": "Paris", "postCode": "75001"}, "comment": "x"})
                .as_object()
                .cloned()
                .unwrap();
        let patch = json!({"address": {"city": "Lyon"}, "comment": ""})
            .as_object()
            .cloned()
            .unwrap();
        overlay(&mut base, patch);
        assert_eq!(
            Value::Object(base),
            json!({"name": "A", "address": {"city": "Lyon", "postCode": "75001"}, "comment": ""})
        );
    }

    #[test]
    fn lists_are_repeated_keys_and_page_sizes_are_clamped() {
        let query = QueryBuilder::new()
            .list("documentTypes", Some(&["invoice", "creditNote"][..]))
            .paged(&PageArgs {
                page: Some(0),
                limit: Some(500),
                search: Some("  ".into()),
                sort_by: None,
                sort_order: Some(SortOrder::Descending),
            })
            .build();
        assert_eq!(
            query,
            vec![
                ("documentTypes".to_owned(), "invoice".to_owned()),
                ("documentTypes".to_owned(), "creditNote".to_owned()),
                ("page".to_owned(), "1".to_owned()),
                ("limit".to_owned(), "100".to_owned()),
                ("sortOrder".to_owned(), "descending".to_owned()),
            ]
        );
    }
}
