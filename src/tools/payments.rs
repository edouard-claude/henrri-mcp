//! Getting paid: payment milestones (échéances), deposits (acomptes),
//! payments and payment modes. 16 routes.
//!
//! A document's schedule is a list of milestones that always adds up to its
//! total: creating, changing or deleting one rebalances the others, which is
//! why those tools answer the whole schedule. Paying a milestone records a
//! payment; Henrri has no route to undo it.

use rmcp::{Json, handler::server::wrapper::Parameters};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    client::Request,
    dates,
    error::{Error, ToolError},
    server::HenrriServer,
    tools::{
        ApiObject, DateArgs, Done, Id, Listing, PageArgs, QueryBuilder, TableArgs,
        require_confirmation, to_api,
    },
};

/// How a difference between the amount received and the amount due is
/// written off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum EcartType {
    /// Early-payment discount granted (over- or underpayment).
    Discount,
    /// The missing amount stays a receivable written off (underpayment).
    Receivable,
    /// Commercial allowance granted on the invoice (underpayment).
    Allowance,
    /// Booked as a profit or a loss (over- or underpayment).
    ProfitLoss,
    /// Company-specific retention (overpayment).
    Specific,
}

/// The filters milestone listings share.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct MilestoneFilters {
    /// true: paid only; false: still due only. Omit for both.
    pub is_paid: Option<bool>,
    /// The milestones of this document only.
    pub document_id: Option<Id>,
    /// true: only unpaid milestones past their due date.
    pub is_overdue: Option<bool>,
}

impl MilestoneFilters {
    fn apply(&self, query: QueryBuilder) -> QueryBuilder {
        query
            .opt("isPaid", self.is_paid)
            .opt("documentId", self.document_id)
            .opt("isOverdue", self.is_overdue)
    }
}

/// Arguments of `list_payment_milestones`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListMilestonesArgs {
    /// Paging, search and sort. `sort_by`: id (default) or date.
    #[serde(flatten)]
    pub page: PageArgs,
    /// Due-date range.
    #[serde(flatten)]
    pub dates: DateArgs,
    /// Paid, document and overdue filters.
    #[serde(flatten)]
    pub filters: MilestoneFilters,
}

/// Arguments of `list_unpaid_milestones_table`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct MilestonesTableArgs {
    /// Paging, search and sort.
    #[serde(flatten)]
    pub page: PageArgs,
    /// Due-date range.
    #[serde(flatten)]
    pub dates: DateArgs,
    /// Document and overdue filters.
    #[serde(flatten)]
    pub filters: MilestoneFilters,
    /// Totals row.
    #[serde(flatten)]
    pub table: TableArgs,
}

/// The fields of a milestone.
#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct MilestoneFields {
    /// The amount due, in euros. This or `percentage`; the amount wins when
    /// both are given.
    pub amount: Option<f64>,
    /// The share of the document's total, in percent: 30 for 30 %.
    pub percentage: Option<f64>,
    /// When it is due: `YYYY-MM-DD`, `in 30 days`…
    pub due_date: Option<String>,
}

/// Arguments of `create_payment_milestone`.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct CreateMilestoneArgs {
    /// The document whose schedule gains a milestone.
    pub document_id: Id,
    /// The milestone. `due_date` and an amount or a percentage are required.
    #[serde(flatten)]
    pub fields: MilestoneFields,
}

/// Arguments of `update_payment_milestone`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateMilestoneArgs {
    /// The milestone to change.
    pub milestone_id: Id,
    /// Only what changes. A new amount or share is taken from the document's
    /// other unpaid milestones.
    #[serde(flatten)]
    pub changes: MilestoneFields,
}

/// Arguments of the tools that act on one milestone.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct MilestoneArgs {
    /// The milestone id, from a document's schedule.
    pub milestone_id: Id,
}

/// Arguments of the tools that need a confirmation on one milestone.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ConfirmMilestoneArgs {
    /// The milestone.
    pub milestone_id: Id,
    /// Must be true: this cannot be undone.
    pub confirm: Option<bool>,
}

/// How a payment was made, for both pay tools.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct PaymentFields {
    /// The payment mode, from `list_payment_modes` (transfer, check, card…).
    pub payment_mode_id: Id,
    /// When the customer paid: `YYYY-MM-DD`, `today`…
    pub date: String,
    /// The bank account it was received on, if tracked.
    pub bank_id: Option<Id>,
    /// What was received, in euros. Omit when it is exactly what was due;
    /// otherwise say what happens to the difference.
    pub amount: Option<f64>,
    /// For an underpayment carried over: when the new milestone for the rest
    /// is due.
    pub ecart_due_date: Option<String>,
    /// For a difference written off rather than carried over: how.
    pub ecart_type: Option<EcartType>,
}

/// Arguments of `pay_payment_milestone`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct PayMilestoneArgs {
    /// The milestone the customer paid.
    pub milestone_id: Id,
    /// The payment.
    #[serde(flatten)]
    pub payment: PaymentFields,
    /// Must be true: the payment is recorded for good.
    pub confirm: Option<bool>,
}

/// Arguments of `pay_payment_milestones`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct PayMilestonesArgs {
    /// The milestones one payment settles, all of the same document.
    pub milestone_ids: Vec<Id>,
    /// The payment.
    #[serde(flatten)]
    pub payment: PaymentFields,
    /// Must be true: the payment is recorded for good.
    pub confirm: Option<bool>,
}

/// Arguments of `create_deposit_settlement`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DepositSettlementArgs {
    /// The quotation milestone whose deposit was paid.
    pub milestone_id: Id,
    /// How the customer paid, from `list_payment_modes`.
    pub payment_mode_id: Id,
    /// When they paid; also the deposit invoice's date.
    pub date: String,
}

/// Arguments of `list_payments`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListPaymentsArgs {
    /// Paging, search and sort.
    #[serde(flatten)]
    pub page: PageArgs,
    /// Payment-date range.
    #[serde(flatten)]
    pub dates: DateArgs,
}

/// Arguments of the tools that read one payment.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct PaymentArgs {
    /// The payment id.
    pub payment_id: Id,
}

/// The ids of a schedule, as `GET /documents/{id}/paymentmilestones` lists it.
fn milestone_ids(schedule: &Value) -> Vec<u64> {
    Listing::from_value(schedule.clone())
        .items
        .iter()
        .filter_map(|m| m.get("id").and_then(Value::as_u64))
        .collect()
}

/// The payment body both pay routes take, with human dates converted.
fn payment_body(payment: &PaymentFields) -> crate::error::Result<Value> {
    if let Some(amount) = payment.amount.filter(|a| *a <= 0.0) {
        return Err(Error::invalid(format!(
            "amount is what was received, a positive number of euros, got {amount}"
        )));
    }
    let mut body = to_api(payment)?;
    body["date"] = Value::String(dates::datetime(&payment.date)?);
    if let Some(raw) = payment.ecart_due_date.as_deref() {
        body["ecartDueDate"] = Value::String(dates::datetime(raw)?);
    }
    Ok(body)
}

fn milestone_body(fields: &MilestoneFields) -> crate::error::Result<Value> {
    if let Some(p) = fields.percentage.filter(|p| !(0.0..=100.0).contains(p)) {
        return Err(Error::invalid(format!(
            "percentage is between 0 and 100, got {p}"
        )));
    }
    let mut body = to_api(fields)?;
    if let Some(raw) = fields.due_date.as_deref() {
        body["dueDate"] = Value::String(dates::datetime(raw)?);
    }
    Ok(body)
}

/// What a milestone `PUT` must not carry back: the payment link and deposit
/// (refused unless unchanged, `PUT_CHANGE_PAYMENT`), the paid flag (refused,
/// `PUT_MARK_AS_PAID`), and the stale twin of a changed amount or share.
fn adjust_milestone(merged: &mut Map<String, Value>, changes: &Map<String, Value>) {
    for key in ["isPaid", "paymentId", "depositDocumentId"] {
        merged.remove(key);
    }
    if changes.contains_key("percentage") && !changes.contains_key("amount") {
        merged.remove("amount");
    } else if changes.contains_key("amount") && !changes.contains_key("percentage") {
        merged.remove("percentage");
    }
}

#[allow(missing_docs, reason = "the router function is generated by the macro")]
#[rmcp::tool_router(router = payments_router, vis = "pub")]
impl HenrriServer {
    /// List the payment milestones of finalized documents: what is due, when,
    /// on which document, and whether it is paid. `is_overdue: true` gives the
    /// late ones, `is_paid: false` everything still to collect. A draft's
    /// schedule is read with `list_document_payment_milestones`.
    #[rmcp::tool(annotations(
        title = "List payment milestones",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_payment_milestones(
        &self,
        Parameters(args): Parameters<ListMilestonesArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let query = args
            .filters
            .apply(QueryBuilder::new().paged(&args.page).dated(&args.dates)?)
            .build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/paymentmilestones").query(query))
            .await?;
        Ok(Json(Listing::from_value(page)))
    }

    /// Add a milestone to a document's schedule: an amount or a share, and a
    /// due date. The schedule is rebalanced, so the answer is every milestone
    /// of the document.
    #[rmcp::tool(annotations(
        title = "Create a payment milestone",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn create_payment_milestone(
        &self,
        Parameters(args): Parameters<CreateMilestoneArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        if args.fields.amount.is_none() && args.fields.percentage.is_none() {
            return Err(Error::invalid("give an amount or a percentage").into());
        }
        if args.fields.due_date.is_none() {
            return Err(Error::invalid("due_date is required").into());
        }
        let mut body = milestone_body(&args.fields)?;
        let wanted_due = body
            .get("dueDate")
            .and_then(Value::as_str)
            .map(str::to_owned);
        body["documentId"] = Value::from(args.document_id.get());
        let schedule_path = format!("/v1/documents/{}/paymentmilestones", args.document_id);
        let before = milestone_ids(&self.client().call(Request::get(&schedule_path)).await?);
        let schedule: Value = self
            .client()
            .call(Request::post("/v1/paymentmilestones").json(body))
            .await?;
        // Henrri ignores dueDate on creation (it honours it on update): set it
        // on the new milestone when it did not stick.
        let created = Listing::from_value(schedule.clone())
            .items
            .into_iter()
            .find(|m| {
                m.get("id")
                    .and_then(Value::as_u64)
                    .is_some_and(|id| !before.contains(&id))
            });
        let Some((created, wanted)) = created.zip(wanted_due) else {
            return Ok(Json(Listing::from_value(schedule)));
        };
        let stored = created
            .get("dueDate")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if stored.get(..10) == wanted.get(..10) {
            return Ok(Json(Listing::from_value(schedule)));
        }
        let Some(id) = created.get("id").and_then(Value::as_u64) else {
            return Ok(Json(Listing::from_value(schedule)));
        };
        let fixed = self
            .modify(
                &format!("/v1/paymentmilestones/{id}"),
                serde_json::json!({ "dueDate": wanted }),
                adjust_milestone,
            )
            .await?;
        Ok(Json(Listing::from_value(fixed).with_note(format!(
            "milestone {id} created; its due date was set in a second call, as Henrri ignores it on creation"
        ))))
    }

    /// The unpaid milestones as Henrri's "unpaid milestones" list shows them:
    /// the columns the user chose (customer, document, due date, amount…),
    /// already formatted, one object per row, with an optional totals row.
    #[rmcp::tool(annotations(
        title = "List unpaid milestones as a table",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_unpaid_milestones_table(
        &self,
        Parameters(args): Parameters<MilestonesTableArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let query = args
            .filters
            .apply(
                QueryBuilder::new()
                    .paged(&args.page)
                    .dated(&args.dates)?
                    .table(&args.table),
            )
            .build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/paymentmilestones/with-selected-fields").query(query))
            .await?;
        Ok(Json(Listing::from_table(page)))
    }

    /// Read one milestone: amount, share, due date, paid, payment, deposit
    /// document.
    #[rmcp::tool(annotations(
        title = "Get a payment milestone",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_payment_milestone(
        &self,
        Parameters(args): Parameters<MilestoneArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let milestone: Value = self
            .client()
            .call(Request::get(format!(
                "/v1/paymentmilestones/{}",
                args.milestone_id
            )))
            .await?;
        Ok(Json(ApiObject::from_value(milestone)))
    }

    /// Change a milestone's amount, share or due date. The difference moves to
    /// the document's other unpaid milestones, so the answer is the whole
    /// schedule. A milestone is never marked paid here: use the pay tools.
    #[rmcp::tool(annotations(
        title = "Update a payment milestone",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn update_payment_milestone(
        &self,
        Parameters(args): Parameters<UpdateMilestoneArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let schedule = self
            .modify(
                &format!("/v1/paymentmilestones/{}", args.milestone_id),
                milestone_body(&args.changes)?,
                adjust_milestone,
            )
            .await?;
        Ok(Json(Listing::from_value(schedule)))
    }

    /// Remove a milestone; its amount goes back to the last one still due.
    /// Refused for a paid milestone, one inherited from the quote, or the last
    /// one due. Refuses to run without `confirm: true`.
    #[rmcp::tool(annotations(
        title = "Delete a payment milestone",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn delete_payment_milestone(
        &self,
        Parameters(args): Parameters<ConfirmMilestoneArgs>,
    ) -> Result<Json<Done>, ToolError> {
        require_confirmation(
            args.confirm,
            &format!(
                "milestone {} would be removed from its schedule",
                args.milestone_id
            ),
        )?;
        let response: Value = self
            .client()
            .call(Request::delete(format!(
                "/v1/paymentmilestones/{}",
                args.milestone_id
            )))
            .await?;
        Ok(Json(Done::new(
            format!("milestone {} deleted", args.milestone_id),
            response,
        )))
    }

    /// Record that the customer paid a milestone: it becomes paid and the
    /// matching payment is created. For a different amount, say what happens
    /// to the difference (`ecart_due_date` or `ecart_type`). Cannot be undone:
    /// refuses to run without `confirm: true`.
    #[rmcp::tool(annotations(
        title = "Pay a payment milestone",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn pay_payment_milestone(
        &self,
        Parameters(args): Parameters<PayMilestoneArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        require_confirmation(
            args.confirm,
            &format!(
                "a payment{} would be recorded on milestone {}, dated {}",
                args.payment
                    .amount
                    .map(|a| format!(" of {a:.2} €"))
                    .unwrap_or_default(),
                args.milestone_id,
                args.payment.date
            ),
        )?;
        let payment: Value = self
            .client()
            .call(
                Request::post(format!("/v1/paymentmilestones/{}/pay", args.milestone_id))
                    .json(payment_body(&args.payment)?),
            )
            .await?;
        Ok(Json(ApiObject::from_value(payment)))
    }

    /// Record one payment settling several milestones of the same document at
    /// once. Without `amount`, their exact sum. Cannot be undone: refuses to
    /// run without `confirm: true`.
    #[rmcp::tool(annotations(
        title = "Pay several payment milestones",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn pay_payment_milestones(
        &self,
        Parameters(args): Parameters<PayMilestonesArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        if args.milestone_ids.is_empty() {
            return Err(Error::invalid("milestone_ids must hold at least one id").into());
        }
        let ids: Vec<String> = args.milestone_ids.iter().map(ToString::to_string).collect();
        require_confirmation(
            args.confirm,
            &format!(
                "one payment{} would be recorded settling milestones {}, dated {}",
                args.payment
                    .amount
                    .map(|a| format!(" of {a:.2} €"))
                    .unwrap_or_default(),
                ids.join(", "),
                args.payment.date
            ),
        )?;
        let mut body = payment_body(&args.payment)?;
        body["milestoneIds"] =
            serde_json::to_value(&args.milestone_ids).map_err(|source| Error::Decode {
                endpoint: "milestone_ids".into(),
                source,
            })?;
        let payment: Value = self
            .client()
            .call(Request::post("/v1/paymentmilestones/pay").json(body))
            .await?;
        Ok(Json(ApiObject::from_value(payment)))
    }

    /// The payment that settled a milestone. A milestone still due has none
    /// (404).
    #[rmcp::tool(annotations(
        title = "Get a milestone's payment",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_milestone_payment(
        &self,
        Parameters(args): Parameters<MilestoneArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let payment: Value = self
            .client()
            .call(Request::get(format!(
                "/v1/paymentmilestones/{}/payment",
                args.milestone_id
            )))
            .await?;
        Ok(Json(ApiObject::from_value(payment)))
    }

    /// Create the deposit request (demande d'acompte) to send the customer for
    /// a milestone of a finalized, not yet invoiced quotation. It is a draft:
    /// finalize it, then send it. Needs Henrri's advanced deposit option.
    #[rmcp::tool(annotations(
        title = "Create a deposit request",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn create_deposit_request(
        &self,
        Parameters(args): Parameters<MilestoneArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let document: Value = self
            .client()
            .call(Request::post(format!(
                "/v1/paymentmilestones/{}/deposit-request",
                args.milestone_id
            )))
            .await?;
        Ok(Json(ApiObject::from_value(document)))
    }

    /// Create the deposit invoice (facture d'acompte) for a deposit the
    /// customer already paid, on a milestone of a finalized quotation. It is a
    /// draft; the payment is recorded when it is finalized.
    #[rmcp::tool(annotations(
        title = "Create a deposit settlement",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn create_deposit_settlement(
        &self,
        Parameters(args): Parameters<DepositSettlementArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let body = serde_json::json!({
            "paymentModeId": args.payment_mode_id.get(),
            "date": dates::datetime(&args.date)?,
        });
        let document: Value = self
            .client()
            .call(
                Request::post(format!(
                    "/v1/paymentmilestones/{}/deposit-settlement",
                    args.milestone_id
                ))
                .json(body),
            )
            .await?;
        Ok(Json(ApiObject::from_value(document)))
    }

    /// Cancel the draft deposit document of a milestone: the document is
    /// deleted and the milestone detached from it. Prefer it to
    /// `delete_document` for deposits. Refuses to run without `confirm: true`.
    #[rmcp::tool(annotations(
        title = "Cancel a deposit",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn cancel_deposit(
        &self,
        Parameters(args): Parameters<ConfirmMilestoneArgs>,
    ) -> Result<Json<Done>, ToolError> {
        require_confirmation(
            args.confirm,
            &format!(
                "the deposit document of milestone {} would be deleted",
                args.milestone_id
            ),
        )?;
        let response: Value = self
            .client()
            .call(Request::delete(format!(
                "/v1/paymentmilestones/{}/deposit",
                args.milestone_id
            )))
            .await?;
        Ok(Json(Done::new(
            format!("deposit of milestone {} cancelled", args.milestone_id),
            response,
        )))
    }

    /// The payment modes of the company (cash, check, card, transfer, direct
    /// debit, PayPal…), with the ids the pay tools take.
    #[rmcp::tool(annotations(
        title = "List payment modes",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_payment_modes(&self) -> Result<Json<Listing>, ToolError> {
        let list: Value = self.client().call(Request::get("/v1/paymentmodes")).await?;
        Ok(Json(Listing::from_value(list)))
    }

    /// List recorded payments, page by page: amount, date, mode, bank, check
    /// number.
    #[rmcp::tool(annotations(
        title = "List payments",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_payments(
        &self,
        Parameters(args): Parameters<ListPaymentsArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        // This route spells its search parameter SearchString.
        let mut page = args.page;
        let search = page.search.take();
        let query = QueryBuilder::new()
            .opt("SearchString", search.as_deref().map(str::trim))
            .paged(&page)
            .dated(&args.dates)?
            .build();
        let list: Value = self
            .client()
            .call(Request::get("/v1/payments").query(query))
            .await?;
        Ok(Json(Listing::from_value(list)))
    }

    /// Read one payment: amount in euros, date, payment mode, bank, check
    /// number.
    #[rmcp::tool(annotations(
        title = "Get a payment",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_payment(
        &self,
        Parameters(args): Parameters<PaymentArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let payment: Value = self
            .client()
            .call(Request::get(format!("/v1/payments/{}", args.payment_id)))
            .await?;
        Ok(Json(ApiObject::from_value(payment)))
    }

    /// The milestones a payment settled, with their documents.
    #[rmcp::tool(annotations(
        title = "List a payment's milestones",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_payment_settled_milestones(
        &self,
        Parameters(args): Parameters<PaymentArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let list: Value = self
            .client()
            .call(Request::get(format!(
                "/v1/payments/{}/milestones",
                args.payment_id
            )))
            .await?;
        Ok(Json(Listing::from_value(list)))
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

    #[test]
    fn a_payment_body_has_datetimes_and_camel_case() {
        let payment: PaymentFields = serde_json::from_value(json!({
            "payment_mode_id": 4, "date": "2026-11-09", "amount": 100,
            "ecart_due_date": "2026-12-09", "ecart_type": "profitLoss"
        }))
        .unwrap();
        let body = payment_body(&payment).unwrap();
        assert_eq!(
            body,
            json!({
                "paymentModeId": 4, "date": "2026-11-09T00:00:00", "amount": 100.0,
                "ecartDueDate": "2026-12-09T00:00:00", "ecartType": "profitLoss"
            })
        );
    }

    #[test]
    fn a_milestone_update_drops_the_stale_twin_and_the_payment_link() {
        let mut merged = json!({"id": 1, "amount": 100, "percentage": 20, "isPaid": false, "paymentId": null, "depositDocumentId": null, "dueDate": "x", "documentId": 9})
            .as_object()
            .cloned()
            .unwrap();
        let changes = json!({"percentage": 30}).as_object().cloned().unwrap();
        adjust_milestone(&mut merged, &changes);
        assert_eq!(
            Value::Object(merged),
            json!({"id": 1, "percentage": 20, "dueDate": "x", "documentId": 9})
        );
    }

    #[test]
    fn a_negative_payment_is_refused() {
        let payment: PaymentFields = serde_json::from_value(json!({
            "payment_mode_id": 4, "date": "today", "amount": -5
        }))
        .unwrap();
        assert!(payment_body(&payment).is_err());
    }
}
