//! The catalogue: items (articles), item categories, text items and units.
//! 17 routes.

use rmcp::{Json, handler::server::wrapper::Parameters};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    client::Request,
    error::{Error, ToolError},
    server::HenrriServer,
    tools::{
        ApiObject, DateArgs, Done, Id, Listing, PageArgs, QueryBuilder, TableArgs,
        customers::YearArgs, require_confirmation, to_api,
    },
};

/// The fields of an item (article).
#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ItemFields {
    /// Reference (SKU). This or `description` is required; the pair is unique.
    pub reference: Option<String>,
    /// Designation. This or `reference` is required.
    pub description: Option<String>,
    /// True when the prices given include tax: then `selling_price_with_tax`
    /// is required, otherwise `selling_price_without_tax`.
    pub is_tax_included: Option<bool>,
    /// Selling price excluding tax, in euros.
    pub selling_price_without_tax: Option<f64>,
    /// Selling price including tax, in euros.
    pub selling_price_with_tax: Option<f64>,
    /// Purchase (cost) price, in euros.
    pub purchase_price: Option<f64>,
    /// VAT rate in percent, 0 to 100: 20 for 20 %. Required at creation.
    pub vat_percent: Option<f64>,
    /// The category, from `list_item_categories`. Required at creation.
    pub item_category_id: Option<Id>,
    /// The unit, from `list_units` (h, kg, m2…).
    pub unit_id: Option<Id>,
}

/// Arguments of `create_item`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct CreateItemArgs {
    /// The item.
    #[serde(flatten)]
    pub item: ItemFields,
}

/// Arguments of `update_item`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateItemArgs {
    /// The item to change.
    pub item_id: Id,
    /// Only the fields to change; the others are kept as they are.
    #[serde(flatten)]
    pub changes: ItemFields,
}

/// Arguments of the tools that read one item.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ItemArgs {
    /// The item id.
    pub item_id: Id,
}

/// Arguments of `delete_item`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DeleteItemArgs {
    /// The item to delete.
    pub item_id: Id,
    /// Must be true: the item leaves the catalogue for good.
    pub confirm: Option<bool>,
}

/// Arguments of the paged catalogue listings.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct CatalogListArgs {
    /// Paging, search and sort.
    #[serde(flatten)]
    pub page: PageArgs,
    /// Creation-date range.
    #[serde(flatten)]
    pub dates: DateArgs,
}

/// Arguments of `list_items_table`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ItemsTableArgs {
    /// Paging, search and sort.
    #[serde(flatten)]
    pub page: PageArgs,
    /// Creation-date range.
    #[serde(flatten)]
    pub dates: DateArgs,
    /// Totals row.
    #[serde(flatten)]
    pub table: TableArgs,
}

/// The fields of a text item.
#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct TextItemFields {
    /// Reference. This or `description` is required; the pair is unique.
    pub reference: Option<String>,
    /// The text printed on the line. This or `reference` is required.
    pub description: Option<String>,
}

/// Arguments of `create_text_item`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct CreateTextItemArgs {
    /// The text item.
    #[serde(flatten)]
    pub text: TextItemFields,
}

/// Arguments of `update_text_item`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateTextItemArgs {
    /// The text item to change.
    pub text_item_id: Id,
    /// Only the fields to change.
    #[serde(flatten)]
    pub changes: TextItemFields,
}

/// Arguments of the tools that read one text item.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct TextItemArgs {
    /// The text item id.
    pub text_item_id: Id,
}

/// Arguments of `delete_text_item`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DeleteTextItemArgs {
    /// The text item to delete.
    pub text_item_id: Id,
    /// Must be true: the text item can no longer be used on new lines.
    pub confirm: Option<bool>,
}

/// Arguments of `create_unit`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct CreateUnitArgs {
    /// The unit's name, e.g. "jour" or "forfait". Unique in the company.
    pub name: String,
}

/// Arguments of `get_unit`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UnitArgs {
    /// The unit id.
    pub unit_id: Id,
}

fn has_text(value: Option<&str>) -> bool {
    value.is_some_and(|s| !s.trim().is_empty())
}

fn check_item(item: &ItemFields) -> crate::error::Result<()> {
    if !has_text(item.reference.as_deref()) && !has_text(item.description.as_deref()) {
        return Err(Error::invalid("an item needs a reference or a description"));
    }
    let Some(vat) = item.vat_percent else {
        return Err(Error::invalid("vat_percent is required (20 for 20 %)"));
    };
    if !(0.0..=100.0).contains(&vat) {
        return Err(Error::invalid(format!(
            "vat_percent is a percentage between 0 and 100, got {vat}"
        )));
    }
    if item.is_tax_included == Some(true) {
        if item.selling_price_with_tax.is_none() {
            return Err(Error::invalid(
                "is_tax_included is true: selling_price_with_tax is required",
            ));
        }
    } else if item.selling_price_without_tax.is_none() {
        return Err(Error::invalid(
            "selling_price_without_tax is required (or set is_tax_included with selling_price_with_tax)",
        ));
    }
    if item.item_category_id.is_none() {
        return Err(Error::invalid(
            "item_category_id is required: list_item_categories gives them",
        ));
    }
    Ok(())
}

/// What an item `PUT` must not carry back: the nested category (it would win
/// over `itemCategoryId`) and the price that is derived from the other one.
fn adjust_item(merged: &mut Map<String, Value>, changes: &Map<String, Value>) {
    merged.remove("itemCategory");
    let tax_included = merged
        .get("isTaxIncluded")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let derived = if tax_included {
        "sellingPriceWithoutTax"
    } else {
        "sellingPriceWithTax"
    };
    if !changes.contains_key(derived) {
        merged.remove(derived);
    }
}

#[allow(missing_docs, reason = "the router function is generated by the macro")]
#[rmcp::tool_router(router = catalog_router, vis = "pub")]
impl HenrriServer {
    /// The item categories (Produits, Services, Mixte… and custom ones), with
    /// their default VAT, margin and hourly rate. `create_item` needs one.
    #[rmcp::tool(annotations(
        title = "List item categories",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_item_categories(
        &self,
        Parameters(args): Parameters<CatalogListArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let query = QueryBuilder::new()
            .paged(&args.page)
            .dated(&args.dates)?
            .build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/itemcategories").query(query))
            .await?;
        Ok(Json(Listing::from_value(page)))
    }

    /// Add an item (article) to the catalogue: a reference or description,
    /// a VAT rate, a price and a category (`list_item_categories`).
    #[rmcp::tool(annotations(
        title = "Create an item",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn create_item(
        &self,
        Parameters(args): Parameters<CreateItemArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        check_item(&args.item)?;
        let created: Value = self
            .client()
            .call(Request::post("/v1/items").json(to_api(&args.item)?))
            .await?;
        Ok(Json(ApiObject::from_value(created)))
    }

    /// List the catalogue's items (articles), page by page, with search and
    /// sorting. Text items are listed by `list_text_items`.
    #[rmcp::tool(annotations(
        title = "List items",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_items(
        &self,
        Parameters(args): Parameters<CatalogListArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let query = QueryBuilder::new()
            .paged(&args.page)
            .dated(&args.dates)?
            .build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/items").query(query))
            .await?;
        Ok(Json(Listing::from_value(page)))
    }

    /// Change some fields of an item (price, VAT, designation…): pass only
    /// what changes, the rest is kept. Lines already on documents keep their
    /// own prices.
    #[rmcp::tool(annotations(
        title = "Update an item",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn update_item(
        &self,
        Parameters(args): Parameters<UpdateItemArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        if let Some(vat) = args
            .changes
            .vat_percent
            .filter(|v| !(0.0..=100.0).contains(v))
        {
            return Err(Error::invalid(format!(
                "vat_percent is a percentage between 0 and 100, got {vat}"
            ))
            .into());
        }
        let updated = self
            .modify(
                &format!("/v1/items/{}", args.item_id),
                to_api(&args.changes)?,
                adjust_item,
            )
            .await?;
        Ok(Json(ApiObject::from_value(updated)))
    }

    /// Remove an item from the catalogue. Refuses to run without
    /// `confirm: true`. Documents already using it are not changed.
    #[rmcp::tool(annotations(
        title = "Delete an item",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn delete_item(
        &self,
        Parameters(args): Parameters<DeleteItemArgs>,
    ) -> Result<Json<Done>, ToolError> {
        let path = format!("/v1/items/{}", args.item_id);
        if args.confirm != Some(true) {
            let what = self.describe(&path, "description").await;
            require_confirmation(
                None,
                &format!("item {}{what} would be deleted", args.item_id),
            )?;
        }
        let response: Value = self.client().call(Request::delete(path)).await?;
        Ok(Json(Done::new(
            format!("item {} deleted", args.item_id),
            response,
        )))
    }

    /// Read one item: reference, designation, prices, VAT, category, unit.
    #[rmcp::tool(annotations(
        title = "Get an item",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_item(
        &self,
        Parameters(args): Parameters<ItemArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let item: Value = self
            .client()
            .call(Request::get(format!("/v1/items/{}", args.item_id)))
            .await?;
        Ok(Json(ApiObject::from_value(item)))
    }

    /// The items used most often on documents: the shortlist to propose when
    /// drafting a quote or an invoice.
    #[rmcp::tool(annotations(
        title = "List most used items",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_most_used_items(&self) -> Result<Json<Listing>, ToolError> {
        let page: Value = self
            .client()
            .call(Request::get("/v1/items/most-used"))
            .await?;
        Ok(Json(Listing::from_value(page)))
    }

    /// The best-selling items over a year (default: this year).
    #[rmcp::tool(annotations(
        title = "List best-selling items",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_best_selling_items(
        &self,
        Parameters(args): Parameters<YearArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let query = QueryBuilder::new().opt("year", args.year).build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/items/best-sales").query(query))
            .await?;
        Ok(Json(Listing::from_value(page)))
    }

    /// Items as Henrri's own item list shows them: the columns the user chose,
    /// already formatted, one object per row, with an optional totals row.
    #[rmcp::tool(annotations(
        title = "List items as a table",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_items_table(
        &self,
        Parameters(args): Parameters<ItemsTableArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let query = QueryBuilder::new()
            .paged(&args.page)
            .dated(&args.dates)?
            .table(&args.table)
            .build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/items/with-selected-fields").query(query))
            .await?;
        Ok(Json(Listing::from_table(page)))
    }

    /// Add a reusable text block for text lines (conditions, warranty…). The
    /// reference and description pair is unique in the company.
    #[rmcp::tool(annotations(
        title = "Create a text item",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn create_text_item(
        &self,
        Parameters(args): Parameters<CreateTextItemArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        if !has_text(args.text.reference.as_deref()) && !has_text(args.text.description.as_deref())
        {
            return Err(Error::invalid("a text item needs a reference or a description").into());
        }
        let created: Value = self
            .client()
            .call(Request::post("/v1/textitems").json(to_api(&args.text)?))
            .await?;
        Ok(Json(ApiObject::from_value(created)))
    }

    /// List the reusable text blocks, page by page (`sort_by`: id, reference,
    /// description).
    #[rmcp::tool(annotations(
        title = "List text items",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_text_items(
        &self,
        Parameters(args): Parameters<CatalogListArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let query = QueryBuilder::new()
            .paged(&args.page)
            .dated(&args.dates)?
            .build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/textitems").query(query))
            .await?;
        Ok(Json(Listing::from_value(page)))
    }

    /// Change a text item's reference or text: pass only what changes.
    #[rmcp::tool(annotations(
        title = "Update a text item",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn update_text_item(
        &self,
        Parameters(args): Parameters<UpdateTextItemArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let updated = self
            .modify(
                &format!("/v1/textitems/{}", args.text_item_id),
                to_api(&args.changes)?,
                |_, _| {},
            )
            .await?;
        Ok(Json(ApiObject::from_value(updated)))
    }

    /// Retire a text item: it can no longer be used on new lines (Henrri keeps
    /// it for existing ones). Refuses to run without `confirm: true`.
    #[rmcp::tool(annotations(
        title = "Delete a text item",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn delete_text_item(
        &self,
        Parameters(args): Parameters<DeleteTextItemArgs>,
    ) -> Result<Json<Done>, ToolError> {
        let path = format!("/v1/textitems/{}", args.text_item_id);
        if args.confirm != Some(true) {
            let what = self.describe(&path, "description").await;
            require_confirmation(
                None,
                &format!("text item {}{what} would be retired", args.text_item_id),
            )?;
        }
        let response: Value = self.client().call(Request::delete(path)).await?;
        Ok(Json(Done::new(
            format!("text item {} retired", args.text_item_id),
            response,
        )))
    }

    /// Read one reusable text block: its reference and its text.
    #[rmcp::tool(annotations(
        title = "Get a text item",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_text_item(
        &self,
        Parameters(args): Parameters<TextItemArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let text: Value = self
            .client()
            .call(Request::get(format!("/v1/textitems/{}", args.text_item_id)))
            .await?;
        Ok(Json(ApiObject::from_value(text)))
    }

    /// Add a unit of measure to the company (a name such as "jour"). Its kind
    /// is set by Henrri; a name that already exists is refused.
    #[rmcp::tool(annotations(
        title = "Create a unit",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn create_unit(
        &self,
        Parameters(args): Parameters<CreateUnitArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let name = args.name.trim();
        if name.is_empty() {
            return Err(Error::invalid("name must not be empty").into());
        }
        // Henrri refuses a unitKind at creation: the name is the whole body.
        let created: Value = self
            .client()
            .call(Request::post("/v1/units").json(json!({ "name": name })))
            .await?;
        if created
            .get("id")
            .and_then(Value::as_u64)
            .is_some_and(|id| id > 0)
        {
            return Ok(Json(ApiObject::from_value(created)));
        }
        // Henrri answers the new unit with id 0: find it by its name.
        let query = QueryBuilder::new().opt("search", Some(name)).build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/units").query(query))
            .await?;
        let found = Listing::from_value(page)
            .items
            .into_iter()
            .find(|u| u.get("name").and_then(Value::as_str) == Some(name));
        Ok(Json(ApiObject::from_value(found.unwrap_or(created))))
    }

    /// The units of measure items can use (h, cm, kg, m2, forfait…), with
    /// their ids, page by page.
    #[rmcp::tool(annotations(
        title = "List units",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_units(
        &self,
        Parameters(args): Parameters<CatalogListArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let query = QueryBuilder::new()
            .paged(&args.page)
            .dated(&args.dates)?
            .build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/units").query(query))
            .await?;
        Ok(Json(Listing::from_value(page)))
    }

    /// Read one unit of measure: its name and kind (hourly, kilogram, custom…).
    #[rmcp::tool(annotations(
        title = "Get a unit",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_unit(
        &self,
        Parameters(args): Parameters<UnitArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let unit: Value = self
            .client()
            .call(Request::get(format!("/v1/units/{}", args.unit_id)))
            .await?;
        Ok(Json(ApiObject::from_value(unit)))
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

    fn item(value: Value) -> ItemFields {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn an_item_says_what_it_lacks() {
        assert!(check_item(&item(json!({"description": "x", "vat_percent": 20, "selling_price_without_tax": 1, "item_category_id": 3}))).is_ok());
        let no_vat = check_item(&item(json!({"description": "x"}))).unwrap_err();
        assert!(no_vat.to_string().contains("vat_percent"));
        let ttc = check_item(&item(json!({"description": "x", "vat_percent": 20, "is_tax_included": true, "item_category_id": 3}))).unwrap_err();
        assert!(ttc.to_string().contains("selling_price_with_tax"));
        assert!(check_item(&item(json!({"description": "x", "vat_percent": 120, "selling_price_without_tax": 1, "item_category_id": 3}))).is_err());
    }

    #[test]
    fn an_item_update_drops_the_derived_price_unless_asked() {
        let mut merged = json!({"isTaxIncluded": false, "sellingPriceWithoutTax": 110, "sellingPriceWithTax": 120, "itemCategory": {}})
            .as_object()
            .cloned()
            .unwrap();
        let changes = json!({"sellingPriceWithoutTax": 110})
            .as_object()
            .cloned()
            .unwrap();
        adjust_item(&mut merged, &changes);
        assert_eq!(
            Value::Object(merged),
            json!({"isTaxIncluded": false, "sellingPriceWithoutTax": 110})
        );
    }
}
