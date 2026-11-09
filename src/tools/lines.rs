//! Document lines: 7 routes.
//!
//! A line is a billable item, a text, or layout (subtotal, total, line break,
//! horizontal line). Groups are defined in Henrri's interface: an existing one
//! can be inserted, not built here. Lines of a finalized document, of a deposit
//! document, or inside a group cannot be changed.

use rmcp::{Json, handler::server::wrapper::Parameters};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    client::Request,
    error::{Error, ToolError},
    server::HenrriServer,
    tools::{
        ApiObject, DateArgs, Done, Id, Listing, PageArgs, QueryBuilder, catalog::ItemFields,
        require_confirmation, to_api,
    },
};

/// What a line is, as `list_document_line_types` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LineKind {
    /// A billable item (article): needs `item_id` or an inline `item`.
    Item,
    /// Free text: needs `description`, or `item_id` of a text item.
    Text,
    /// A subtotal of the lines above.
    Subtotal,
    /// A total.
    Total,
    /// An empty line.
    LineBreak,
    /// A horizontal rule.
    HorizontalLine,
    /// An existing group of items, inserted by its `item_id`.
    Group,
    /// An adjustment line.
    Adjustment,
}

/// The line fields both creation and update take.
#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct LineFields {
    /// Reference printed on the line.
    pub reference: Option<String>,
    /// Designation. For a text line without `item_id`, the text itself (a
    /// text item is created from it).
    pub description: Option<String>,
    /// Quantity. Negative on a credit note.
    pub quantity: Option<f64>,
    /// Unit selling price excluding tax, in euros.
    pub selling_price_without_tax: Option<f64>,
    /// Unit purchase price excluding tax, for the margin.
    pub purchasing_price_without_tax: Option<f64>,
    /// VAT rate in percent: 20 for 20 %.
    pub vat_percent: Option<f64>,
    /// Prices include tax. Must match the document's `isTaxIncludedMode`;
    /// leave it out to inherit it.
    pub is_tax_included: Option<bool>,
    /// The item (article), text item or group this line is made from.
    pub item_id: Option<Id>,
}

/// Arguments of `add_document_line`.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AddLineArgs {
    /// The draft document.
    #[serde(skip_serializing)]
    pub document_id: Id,
    /// What the line is. Give this or `type_id`.
    #[serde(skip_serializing)]
    pub kind: Option<LineKind>,
    /// The line type id from `list_document_line_types`. Give this or `kind`.
    pub type_id: Option<Id>,
    /// The line's content.
    #[serde(flatten)]
    pub fields: LineFields,
    /// An item to create on the fly for an item line, instead of `item_id`:
    /// `vat_percent`, a `reference` or `description`, a price and
    /// `item_category_id` are needed.
    pub item: Option<ItemFields>,
}

/// Arguments of `update_document_line`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateLineArgs {
    /// The draft document.
    pub document_id: Id,
    /// The line to change. Its type cannot change: delete and add instead.
    pub line_id: Id,
    /// Only the fields to change; the others are kept as they are.
    #[serde(flatten)]
    pub changes: LineFields,
}

/// Arguments of the tools that read one line.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct LineArgs {
    /// The document.
    pub document_id: Id,
    /// The line, as `list_document_lines` shows it.
    pub line_id: Id,
}

/// Arguments of `delete_document_line`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DeleteLineArgs {
    /// The draft document.
    pub document_id: Id,
    /// The line to delete. Deleting a group line deletes the whole group.
    pub line_id: Id,
    /// Must be true: the line is deleted for good.
    pub confirm: Option<bool>,
}

/// Arguments of `move_document_line`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct MoveLineArgs {
    /// The draft document.
    pub document_id: Id,
    /// The line to move.
    pub line_id: Id,
    /// Its new position, from 1. Past the end means last.
    pub to: u32,
}

/// Arguments of `list_document_lines`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ListLinesArgs {
    /// The document.
    pub document_id: Id,
    /// Paging and search. Without paging, every line comes at once, in order.
    #[serde(flatten)]
    pub page: PageArgs,
    /// Creation-date range.
    #[serde(flatten)]
    pub dates: DateArgs,
}

/// Refuse a line Henrri would refuse, saying what is missing.
fn check_line(kind: Option<LineKind>, args: &AddLineArgs) -> crate::error::Result<()> {
    if args.item.is_some() && args.fields.item_id.is_some() {
        return Err(Error::invalid("give item_id or an inline item, not both"));
    }
    let has_text = args
        .fields
        .description
        .as_deref()
        .is_some_and(|d| !d.trim().is_empty());
    match kind {
        Some(LineKind::Item) if args.fields.item_id.is_none() && args.item.is_none() => Err(
            Error::invalid("an item line needs item_id (list_items) or an inline item"),
        ),
        Some(LineKind::Text) if !has_text && args.fields.item_id.is_none() => Err(Error::invalid(
            "a text line needs a description, or item_id of a text item (list_text_items)",
        )),
        Some(LineKind::Group) if args.fields.item_id.is_none() => Err(Error::invalid(
            "a group line needs item_id: groups are defined in Henrri and inserted by id",
        )),
        _ => Ok(()),
    }
}

#[allow(missing_docs, reason = "the router function is generated by the macro")]
#[rmcp::tool_router(router = lines_router, vis = "pub")]
impl HenrriServer {
    /// The lines of a document, in order, with their computed totals (excl.
    /// and incl. tax). Without paging, every line comes at once.
    #[rmcp::tool(annotations(
        title = "List a document's lines",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_document_lines(
        &self,
        Parameters(args): Parameters<ListLinesArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let query = QueryBuilder::new()
            .paged(&args.page)
            .dated(&args.dates)?
            .build();
        let page: Value = self
            .client()
            .call(Request::get(format!("/v1/documents/{}/lines", args.document_id)).query(query))
            .await?;
        Ok(Json(Listing::from_value(page)))
    }

    /// Add a line to a draft: an item (`item_id`, or an inline `item` created
    /// on the fly) with its quantity and price, a text (`description`), or
    /// layout (subtotal, total, line break, horizontal line). Name the kind
    /// with `kind`; the matching type id is looked up for you.
    #[rmcp::tool(annotations(
        title = "Add a document line",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn add_document_line(
        &self,
        Parameters(args): Parameters<AddLineArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        check_line(args.kind, &args)?;
        let type_id = match (args.type_id, args.kind) {
            (Some(id), _) => id.get(),
            (None, Some(kind)) => self.line_type_id(kind).await?,
            (None, None) => {
                return Err(Error::invalid(
                    "say what the line is: kind (item, text, subtotal…) or type_id",
                )
                .into());
            }
        };
        let mut body = to_api(&args)?;
        body["typeId"] = json!(type_id);
        let created: Value = self
            .client()
            .call(Request::post(format!("/v1/documents/{}/lines", args.document_id)).json(body))
            .await?;
        Ok(Json(ApiObject::from_value(created)))
    }

    /// Read one line of a document, with its item and computed totals.
    #[rmcp::tool(annotations(
        title = "Get a document line",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_document_line(
        &self,
        Parameters(args): Parameters<LineArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let line: Value = self
            .client()
            .call(Request::get(format!(
                "/v1/documents/{}/lines/{}",
                args.document_id, args.line_id
            )))
            .await?;
        Ok(Json(ApiObject::from_value(line)))
    }

    /// Change some fields of a line (quantity, price, VAT, text…): pass only
    /// what changes, the rest is kept. The totals are recomputed by Henrri.
    #[rmcp::tool(annotations(
        title = "Update a document line",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn update_document_line(
        &self,
        Parameters(args): Parameters<UpdateLineArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let updated = self
            .modify(
                &format!("/v1/documents/{}/lines/{}", args.document_id, args.line_id),
                to_api(&args.changes)?,
                adjust_line,
            )
            .await?;
        Ok(Json(ApiObject::from_value(updated)))
    }

    /// Delete a line of a draft, for good. Refuses to run without
    /// `confirm: true`. A line inside a group cannot be deleted alone.
    #[rmcp::tool(annotations(
        title = "Delete a document line",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn delete_document_line(
        &self,
        Parameters(args): Parameters<DeleteLineArgs>,
    ) -> Result<Json<Done>, ToolError> {
        let path = format!("/v1/documents/{}/lines/{}", args.document_id, args.line_id);
        if args.confirm != Some(true) {
            let what = self.describe(&path, "description").await;
            require_confirmation(
                None,
                &format!(
                    "line {}{what} of document {} would be deleted",
                    args.line_id, args.document_id
                ),
            )?;
        }
        let response: Value = self.client().call(Request::delete(path)).await?;
        Ok(Json(Done::new(
            format!(
                "line {} of document {} deleted",
                args.line_id, args.document_id
            ),
            response,
        )))
    }

    /// Move a line to another position (from 1) in a draft. Returns every line
    /// of the document in its new order.
    #[rmcp::tool(annotations(
        title = "Move a document line",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn move_document_line(
        &self,
        Parameters(args): Parameters<MoveLineArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        if args.to == 0 {
            return Err(Error::invalid("positions start at 1").into());
        }
        let lines: Value = self
            .client()
            .call(
                Request::post(format!(
                    "/v1/documents/{}/lines/{}/move",
                    args.document_id, args.line_id
                ))
                .json(json!({ "to": args.to })),
            )
            .await?;
        Ok(Json(Listing::from_value(lines)))
    }

    /// The line types `add_document_line` accepts, with their ids: item,
    /// text, subtotal, total, group, adjustment, line break, horizontal line.
    #[rmcp::tool(annotations(
        title = "List document line types",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_document_line_types(&self) -> Result<Json<Listing>, ToolError> {
        let list: Value = self
            .client()
            .call(Request::get("/v1/documentlinetypes"))
            .await?;
        Ok(Json(Listing::from_value(list)))
    }
}

impl HenrriServer {
    /// The type id Henrri uses for a kind of line.
    async fn line_type_id(&self, kind: LineKind) -> crate::error::Result<u32> {
        let list: Value = self
            .client()
            .call(Request::get("/v1/documentlinetypes"))
            .await?;
        let wanted = crate::tools::api_name(&kind);
        let types = list
            .get("elements")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        types
            .iter()
            .find(|t| {
                t.get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|k| k.eq_ignore_ascii_case(&wanted))
            })
            .and_then(|t| t.get("id").and_then(Value::as_u64))
            .and_then(|id| u32::try_from(id).ok())
            .ok_or_else(|| {
                let known: Vec<&str> = types
                    .iter()
                    .filter_map(|t| t.get("type").and_then(Value::as_str))
                    .collect();
                Error::invalid(format!(
                    "this company has no {wanted} line type; it has: {}",
                    known.join(", ")
                ))
            })
    }
}

/// What a line `PUT` must not carry back: the nested item (validated as a new
/// item and refused), the type object, the computed totals (refused outside
/// tax-included mode), and the group fields (read-only since October 2026).
fn adjust_line(merged: &mut Map<String, Value>, _changes: &Map<String, Value>) {
    for key in [
        "item",
        "type",
        "totalWithoutTax",
        "totalWithTax",
        "groupId",
        "isMemberOfAGroup",
    ] {
        merged.remove(key);
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

    fn add(value: Value) -> AddLineArgs {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn the_line_body_carries_neither_the_document_nor_the_kind() {
        let args = add(json!({
            "document_id": 9, "kind": "item", "item_id": 24, "quantity": 2,
            "item": null
        }));
        let body = to_api(&args).unwrap();
        assert_eq!(body, json!({"itemId": 24, "quantity": 2.0}));
    }

    #[test]
    fn a_line_says_what_it_lacks() {
        let empty_item = add(json!({"document_id": 9, "kind": "item"}));
        assert!(check_line(Some(LineKind::Item), &empty_item).is_err());
        let empty_text = add(json!({"document_id": 9, "kind": "text", "description": " "}));
        assert!(check_line(Some(LineKind::Text), &empty_text).is_err());
        let both = add(json!({
            "document_id": 9, "kind": "item", "item_id": 1,
            "item": {"description": "x", "vat_percent": 20}
        }));
        assert!(check_line(Some(LineKind::Item), &both).is_err());
        let rule = add(json!({"document_id": 9, "kind": "horizontalLine"}));
        assert!(check_line(Some(LineKind::HorizontalLine), &rule).is_ok());
    }

    #[test]
    fn a_line_update_drops_what_henrri_refuses_back() {
        let mut merged = json!({
            "id": 1, "quantity": 4, "item": {"id": 2}, "type": {"id": 3},
            "totalWithoutTax": 1, "totalWithTax": 1.2, "groupId": null, "isMemberOfAGroup": false
        })
        .as_object()
        .cloned()
        .unwrap();
        adjust_line(&mut merged, &Map::new());
        assert_eq!(Value::Object(merged), json!({"id": 1, "quantity": 4}));
    }
}
