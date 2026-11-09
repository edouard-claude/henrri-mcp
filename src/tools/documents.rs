//! Documents: quotes, invoices, credit notes, delivery notes, work orders…
//! 21 routes, PDFs included.
//!
//! A document is created empty, as a **draft**; lines are added with the line
//! tools; `finalize_document` gives it its definitive number and locks it.
//! Finalizing an accounting document (invoice, credit note) costs 8 credits
//! and can never be undone: anti-fraud rules forbid deleting it afterwards.

use std::path::{Path, PathBuf};

use chrono::{Local, Offset};
use rmcp::{Json, handler::server::wrapper::Parameters};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    client::Request,
    dates,
    error::{Error, ToolError},
    server::HenrriServer,
    tools::{
        ApiObject, DateArgs, Done, Id, Listing, PageArgs, QueryBuilder, TableArgs,
        customers::AddressInput, require_confirmation, to_api,
    },
};

/// A document kind, as the `documentTypes` filter spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DocumentKind {
    /// Facture.
    Invoice,
    /// Avoir.
    CreditNote,
    /// Bon de livraison.
    DeliveryNote,
    /// Devis.
    Quotation,
    /// Ordre de travail.
    WorkOrder,
    /// Facture de situation.
    ProgressInvoice,
    /// Bon de commande.
    Order,
    /// Avenant.
    Amendment,
    /// Divers.
    Miscellaneous,
    /// Récapitulatif.
    Summary,
    /// Facture d'acompte.
    DepositInvoice,
    /// Reçu / demande d'acompte.
    DepositReceipt,
}

/// Finalized or draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DocumentState {
    /// Locked, with a definitive number.
    Finalized,
    /// A draft, still editable.
    Pending,
}

/// The filters `list_documents` and `list_documents_table` share.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct DocumentFilters {
    /// Keep documents of these kinds only, e.g. `["invoice", "creditNote"]`.
    pub document_types: Option<Vec<DocumentKind>>,
    /// `finalized` or `pending` (drafts). Omit for both.
    pub state: Option<DocumentState>,
    /// Same as `state`, as a boolean: true for finalized, false for drafts.
    /// Do not combine with `state`.
    pub finalized: Option<bool>,
    /// Only documents whose id is strictly greater: the simplest way to fetch
    /// what is new since the last call.
    pub min_id: Option<Id>,
}

impl DocumentFilters {
    fn apply(&self, query: QueryBuilder) -> crate::error::Result<QueryBuilder> {
        if self.state.is_some() && self.finalized.is_some() {
            return Err(Error::invalid(
                "pass state or finalized, not both: they are the same filter",
            ));
        }
        Ok(query
            .list("documentTypes", self.document_types.as_deref())
            .opt_enum("state", self.state)
            .opt("finalized", self.finalized)
            .opt("minId", self.min_id))
    }
}

/// Arguments of `list_documents`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListDocumentsArgs {
    /// Paging, search and sort. `sort_by`: id (default), identity (the number)
    /// or date.
    #[serde(flatten)]
    pub page: PageArgs,
    /// Document-date range.
    #[serde(flatten)]
    pub dates: DateArgs,
    /// Kind, state and id filters.
    #[serde(flatten)]
    pub filters: DocumentFilters,
}

/// Arguments of `list_documents_table`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct DocumentsTableArgs {
    /// Paging, search and sort.
    #[serde(flatten)]
    pub page: PageArgs,
    /// Document-date range.
    #[serde(flatten)]
    pub dates: DateArgs,
    /// Kind, state and id filters.
    #[serde(flatten)]
    pub filters: DocumentFilters,
    /// Totals row.
    #[serde(flatten)]
    pub table: TableArgs,
}

/// The document fields both creation and update take.
#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct DocumentFields {
    /// Title (objet) of the document.
    pub title: Option<String>,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// The document's date: `YYYY-MM-DD`, `today`… or a full datetime.
    pub date: Option<String>,
    /// Text printed in the footer: payment instructions, legal mentions.
    pub footer_text: Option<String>,
    /// The bank account printed for payment.
    pub bank_account_label: Option<String>,
    /// A label (étiquette) to file the document under.
    pub label_id: Option<Id>,
    /// True when prices are entered tax included. Every line follows it.
    pub is_tax_included_mode: Option<bool>,
    /// The customer's address as printed on this document, when it differs
    /// from the customer's own.
    pub customer_address: Option<AddressInput>,
}

/// Arguments of `create_document`.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct CreateDocumentArgs {
    /// The document type id, from `list_document_types` (invoice, quote…).
    /// Deposit requests and receipts are created from a payment milestone.
    pub document_type_id: Id,
    /// The customer, from `list_customers`.
    pub customer_id: Id,
    /// Everything else.
    #[serde(flatten)]
    pub fields: DocumentFields,
}

/// What an update changes on a document.
#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct DocumentChanges {
    /// Another document type.
    pub document_type_id: Option<Id>,
    /// Another customer.
    pub customer_id: Option<Id>,
    /// Everything else.
    #[serde(flatten)]
    pub fields: DocumentFields,
}

/// Arguments of `update_document`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateDocumentArgs {
    /// The draft to change.
    pub document_id: Id,
    /// Only the fields to change; the others are kept as they are.
    #[serde(flatten)]
    pub changes: DocumentChanges,
}

/// Arguments of the tools that read one document.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DocumentArgs {
    /// The document id (not its number: `list_documents` gives both).
    pub document_id: Id,
}

/// Arguments of `delete_document`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DeleteDocumentArgs {
    /// The document to delete.
    pub document_id: Id,
    /// Must be true: the document is deleted for good.
    pub confirm: Option<bool>,
}

/// Arguments of `finalize_document`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct FinalizeArgs {
    /// The draft to finalize.
    pub document_id: Id,
    /// The date of the final document, e.g. to invoice an earlier period.
    /// Default: the draft's date. It cannot precede the last issued document.
    pub date: Option<String>,
    /// Henrri asks for an explicit agreement before the first accounting
    /// document of a month (`FIRST_ACCOUNTING_DOCUMENT_MONTH`): set it then.
    /// Sent as the `confirmFirstOfMonth` query parameter.
    pub confirm_first_of_month: Option<bool>,
    /// Must be true: a finalized document is locked forever, and costs credits.
    pub confirm: Option<bool>,
}

/// Arguments of `validate_quotation`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ValidateArgs {
    /// The finalized quotation the customer accepted.
    pub document_id: Id,
    /// Email of the person who accepted (signed) the quote.
    pub email: String,
    /// Their first name.
    pub first_name: String,
    /// Their last name.
    pub last_name: String,
    /// The IP address they signed from, as the signature tool recorded it.
    pub ip: String,
    /// When they accepted. Default: now.
    pub validation_date: Option<String>,
    /// Their offset from UTC, in seconds (3600 for Paris in winter). Default:
    /// this machine's.
    pub time_offset: Option<i32>,
    /// Must be true: the acceptance is recorded for good, and costs 2 credits.
    pub confirm: Option<bool>,
}

/// Arguments of `set_payment_terms_display`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct PaymentTermsChanges {
    /// Show each instalment's amount.
    pub show_amounts: Option<bool>,
    /// Show each instalment's share of the total.
    pub show_percentages: Option<bool>,
    /// Show each instalment's condition text.
    pub show_conditions: Option<bool>,
    /// Show each instalment's due date.
    pub show_due_dates: Option<bool>,
    /// A custom text replacing the generated one. Empty: back to generated.
    pub due_label: Option<String>,
}

/// Arguments of `set_payment_terms_display`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct PaymentTermsArgs {
    /// The draft document.
    pub document_id: Id,
    /// Only the switches to change; the others are kept.
    #[serde(flatten)]
    pub changes: PaymentTermsChanges,
}

/// Arguments of `get_document_pdf`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct PdfArgs {
    /// The document.
    pub document_id: Id,
    /// Embed the Factur-X (EN 16931) data, for e-invoicing platforms. For
    /// invoices only, with the customer's address, SIRET or VAT number and the
    /// VAT rates filled in.
    pub is_factur_x: Option<bool>,
    /// The file name to save under, in the download directory. Default: the
    /// name Henrri suggests.
    pub file_name: Option<String>,
}

/// Arguments of `download_document_pdf_file`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct PdfFileArgs {
    /// The document.
    pub document_id: Id,
    /// The file's GUID: the last segment of the URL `create_document_pdf_url`
    /// returned. It expires 10 minutes after creation.
    pub guid: String,
    /// The file name to save under, in the download directory.
    pub file_name: Option<String>,
}

/// A file written to disk.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SavedFile {
    /// Where the file is, on the machine running this server.
    pub path: String,
    /// Its name.
    pub file_name: String,
    /// Its size, in bytes.
    pub bytes: usize,
    /// Its media type, as Henrri labelled it.
    pub content_type: String,
    /// Also readable as an MCP resource at this URI.
    pub resource_uri: String,
}

/// What `create_document_pdf_url` answers.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct PdfUrl {
    /// The temporary URL. Downloading it needs the bearer token: use
    /// `download_document_pdf_file` with `guid` instead.
    pub download_url: Option<String>,
    /// The file's GUID, for `download_document_pdf_file`.
    pub guid: Option<String>,
    /// When the file expires (UTC), 10 minutes after creation.
    pub expires_at: Option<String>,
    /// The file's name.
    pub file_name: Option<String>,
}

/// Keep a file name a file name: no directory, no traversal, no control
/// characters, and a `.pdf` extension.
pub fn safe_file_name(raw: &str, fallback: &str) -> String {
    let base = Path::new(raw.trim())
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let name = if cleaned.is_empty() {
        fallback
    } else {
        cleaned
    };
    if Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
    {
        name.to_owned()
    } else {
        format!("{name}.pdf")
    }
}

impl HenrriServer {
    /// Download a PDF route and write it in the download directory.
    async fn save_pdf(
        &self,
        request: Request,
        document_id: Id,
        file_name: Option<&str>,
    ) -> crate::error::Result<SavedFile> {
        let label = request.label();
        let response = self.client().send(request).await?;
        if !response.bytes.starts_with(b"%PDF") {
            return Err(Error::invalid(format!(
                "{label} answered {} bytes of {}, not a PDF",
                response.bytes.len(),
                response.content_type
            )));
        }
        let fallback = format!("document-{document_id}.pdf");
        let name = safe_file_name(
            file_name
                .or(response.file_name.as_deref())
                .unwrap_or(&fallback),
            &fallback,
        );
        let dir = &self.config().download_dir;
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|source| Error::Io {
                path: dir.display().to_string(),
                source,
            })?;
        let path: PathBuf = dir.join(&name);
        tokio::fs::write(&path, &response.bytes)
            .await
            .map_err(|source| Error::Io {
                path: path.display().to_string(),
                source,
            })?;
        Ok(SavedFile {
            path: path.display().to_string(),
            file_name: name,
            bytes: response.bytes.len(),
            content_type: response.content_type,
            resource_uri: format!("henrri://documents/{document_id}/pdf"),
        })
    }
}

/// Replace a human date in a body by the datetime the API wants.
fn normalize_date(body: &mut Value) -> crate::error::Result<()> {
    if let Some(raw) = body.get("date").and_then(Value::as_str) {
        let datetime = dates::datetime(raw)?;
        body["date"] = Value::String(datetime);
    }
    Ok(())
}

#[allow(missing_docs, reason = "the router function is generated by the macro")]
#[rmcp::tool_router(router = documents_router, vis = "pub")]
impl HenrriServer {
    /// List documents (quotes, invoices, credit notes…), page by page: filter
    /// by kind (`document_types`), draft or finalized (`state`), date range
    /// (`period`), free text, and `min_id` to get only what is new. Amounts are
    /// in euros (`priceBeforeTax`, `taxAmount`, `priceAfterTax`).
    #[rmcp::tool(annotations(
        title = "List documents",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_documents(
        &self,
        Parameters(args): Parameters<ListDocumentsArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let query = args
            .filters
            .apply(QueryBuilder::new().paged(&args.page).dated(&args.dates)?)?
            .build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/documents").query(query))
            .await?;
        Ok(Json(Listing::from_value(page)))
    }

    /// Create an empty draft document of a type (`list_document_types`) for a
    /// customer. Then add its lines with `add_document_line`, and finalize it
    /// with `finalize_document`. Returns the draft with its id.
    #[rmcp::tool(annotations(
        title = "Create a document",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn create_document(
        &self,
        Parameters(args): Parameters<CreateDocumentArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let mut body = to_api(&args)?;
        normalize_date(&mut body)?;
        let created: Value = self
            .client()
            .call(Request::post("/v1/documents").json(body))
            .await?;
        Ok(Json(ApiObject::from_value(created)))
    }

    /// Documents as Henrri's own document list shows them: the columns the
    /// user chose (type, date, number, customer, amounts…), already formatted,
    /// one object per row, with an optional totals row.
    #[rmcp::tool(annotations(
        title = "List documents as a table",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_documents_table(
        &self,
        Parameters(args): Parameters<DocumentsTableArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let query = args
            .filters
            .apply(
                QueryBuilder::new()
                    .paged(&args.page)
                    .dated(&args.dates)?
                    .table(&args.table),
            )?
            .build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/documents/with-selected-fields").query(query))
            .await?;
        Ok(Json(Listing::from_table(page)))
    }

    /// The next batch of up to 50 finalized quotes, after the last one this
    /// API account was given. Henrri keeps the cursor: each call moves it
    /// forward, so two calls never return the same quotes. Meant for a sync.
    #[rmcp::tool(annotations(
        title = "Fetch the next quote batch",
        read_only_hint = true,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn next_quote_batch(&self) -> Result<Json<Listing>, ToolError> {
        let page: Value = self
            .client()
            .call(Request::get("/v1/documents/next-quote-batch"))
            .await?;
        Ok(Json(Listing::from_value(page).with_note(
            "the cursor moved: the next call returns the quotes after these",
        )))
    }

    /// Read one document: number, type, state, customer, amounts, dates,
    /// validation. Lines are not included: use `get_document_with_all` or
    /// `list_document_lines`.
    #[rmcp::tool(annotations(
        title = "Get a document",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_document(
        &self,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let document: Value = self
            .client()
            .call(Request::get(format!("/v1/documents/{}", args.document_id)))
            .await?;
        Ok(Json(ApiObject::from_value(document)))
    }

    /// Change some fields of a draft: pass only what changes, the rest is
    /// kept (the tool reads the draft and writes the whole back). A finalized
    /// document, a deposit request or a deposit receipt cannot be changed.
    #[rmcp::tool(annotations(
        title = "Update a document",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn update_document(
        &self,
        Parameters(args): Parameters<UpdateDocumentArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let mut patch = to_api(&args.changes)?;
        normalize_date(&mut patch)?;
        let updated = self
            .modify(
                &format!("/v1/documents/{}", args.document_id),
                patch,
                adjust_document,
            )
            .await?;
        Ok(Json(ApiObject::from_value(updated)))
    }

    /// Delete a document, for good. Refuses to run without `confirm: true`.
    /// Henrri refuses it for a finalized invoice or credit note (anti-fraud
    /// rules), an invoiced quote, or a document with payments.
    #[rmcp::tool(annotations(
        title = "Delete a document",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn delete_document(
        &self,
        Parameters(args): Parameters<DeleteDocumentArgs>,
    ) -> Result<Json<Done>, ToolError> {
        let path = format!("/v1/documents/{}", args.document_id);
        if args.confirm != Some(true) {
            let number = self.describe(&path, "identity").await;
            require_confirmation(
                None,
                &format!("document {}{number} would be deleted", args.document_id),
            )?;
        }
        let response: Value = self.client().call(Request::delete(path)).await?;
        Ok(Json(Done::new(
            format!("document {} deleted", args.document_id),
            response,
        )))
    }

    /// Read a document with everything needed to render it: lines, customer,
    /// company, tax details per rate and display settings.
    #[rmcp::tool(annotations(
        title = "Get a document with all its elements",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_document_with_all(
        &self,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let document: Value = self
            .client()
            .call(Request::get(format!(
                "/v1/documents/{}/with-all",
                args.document_id
            )))
            .await?;
        Ok(Json(ApiObject::from_value(document)))
    }

    /// Read a document with all its elements included, as the `all-included`
    /// route returns it: the same content as `get_document_with_all`, from
    /// Henrri's second route for it.
    #[rmcp::tool(annotations(
        title = "Get a document, all included",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_document_all_included(
        &self,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let document: Value = self
            .client()
            .call(Request::get(format!(
                "/v1/documents/{}/all-included",
                args.document_id
            )))
            .await?;
        Ok(Json(ApiObject::from_value(document)))
    }

    /// The VAT breakdown of a document: for each rate, the base, the tax and
    /// the total, in euros.
    #[rmcp::tool(annotations(
        title = "Get a document's tax details",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_document_tax_details(
        &self,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let details: Value = self
            .client()
            .call(Request::get(format!(
                "/v1/documents/{}/tax-details",
                args.document_id
            )))
            .await?;
        Ok(Json(Listing::from_value(details)))
    }

    /// Record that the customer accepted (signed) a finalized quotation: who,
    /// when, from which IP. Costs 2 credits and cannot be undone: refuses to
    /// run without `confirm: true`.
    #[rmcp::tool(annotations(
        title = "Validate a quotation",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn validate_quotation(
        &self,
        Parameters(args): Parameters<ValidateArgs>,
    ) -> Result<Json<Done>, ToolError> {
        require_confirmation(
            args.confirm,
            &format!(
                "quotation {} would be recorded as accepted by {} {} <{}>, costing 2 credits",
                args.document_id, args.first_name, args.last_name, args.email
            ),
        )?;
        for (name, value) in [
            ("email", &args.email),
            ("first_name", &args.first_name),
            ("last_name", &args.last_name),
            ("ip", &args.ip),
        ] {
            if value.trim().is_empty() {
                return Err(Error::invalid(format!("{name} must not be empty")).into());
            }
        }
        let validation_date = match args.validation_date.as_deref() {
            Some(raw) => dates::datetime(raw)?,
            None => Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        };
        let time_offset = args
            .time_offset
            .unwrap_or_else(|| Local::now().offset().fix().local_minus_utc());
        let body = json!({
            "email": args.email.trim(),
            "firstName": args.first_name.trim(),
            "lastName": args.last_name.trim(),
            "validationDate": validation_date,
            "timeOffset": time_offset,
            "ip": args.ip.trim(),
        });
        let response: Value = self
            .client()
            .call(Request::post(format!("/v1/documents/{}/validate", args.document_id)).json(body))
            .await?;
        Ok(Json(Done::new(
            format!("quotation {} validated", args.document_id),
            response,
        )))
    }

    /// Download a document as a PDF and save it in the download directory
    /// (HENRRI_DOWNLOAD_DIR). A finalized document renders as it was locked; a
    /// draft as it is now. `is_factur_x` embeds the EN 16931 e-invoice data.
    #[rmcp::tool(annotations(
        title = "Download a document PDF",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_document_pdf(
        &self,
        Parameters(args): Parameters<PdfArgs>,
    ) -> Result<Json<SavedFile>, ToolError> {
        let query = QueryBuilder::new()
            .opt("isFacturX", args.is_factur_x)
            .build();
        let saved = self
            .save_pdf(
                Request::get(format!("/v1/documents/{}/pdf", args.document_id))
                    .query(query)
                    .binary("application/pdf"),
                args.document_id,
                args.file_name.as_deref(),
            )
            .await?;
        Ok(Json(saved))
    }

    /// Download a PDF that `create_document_pdf_url` generated, by its GUID,
    /// and save it in the download directory. The file expires 10 minutes
    /// after it was generated.
    #[rmcp::tool(annotations(
        title = "Download a generated PDF file",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn download_document_pdf_file(
        &self,
        Parameters(args): Parameters<PdfFileArgs>,
    ) -> Result<Json<SavedFile>, ToolError> {
        let guid = args.guid.trim();
        if guid.is_empty() || !guid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Err(Error::invalid(format!(
                "{guid:?} is not a file GUID: pass the last segment of the download URL"
            ))
            .into());
        }
        let saved = self
            .save_pdf(
                Request::get(format!(
                    "/v1/documents/{}/pdf/files/{guid}",
                    args.document_id
                ))
                .binary("application/pdf, application/octet-stream"),
                args.document_id,
                args.file_name.as_deref(),
            )
            .await?;
        Ok(Json(saved))
    }

    /// Generate a document's PDF on Henrri's side and get a temporary download
    /// URL, valid 10 minutes and only with the bearer token. To get the file
    /// itself, `get_document_pdf` is simpler; this suits handing the GUID to
    /// `download_document_pdf_file` later.
    #[rmcp::tool(annotations(
        title = "Generate a PDF download URL",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn create_document_pdf_url(
        &self,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<Json<PdfUrl>, ToolError> {
        let answer: Value = self
            .client()
            .call(Request::post(format!("/v1/documents/{}/pdf/url", args.document_id)).slow())
            .await?;
        let text = |key: &str| answer.get(key).and_then(Value::as_str).map(str::to_owned);
        let download_url = text("downloadUrl");
        let guid = download_url
            .as_deref()
            .and_then(|url| url.split('?').next())
            .and_then(|url| url.rsplit('/').next())
            .filter(|g| !g.is_empty())
            .map(str::to_owned);
        Ok(Json(PdfUrl {
            download_url,
            guid,
            expires_at: text("expiresAt"),
            file_name: text("fileName"),
        }))
    }

    /// The columns a document's lines are printed with (reference,
    /// designation, quantity, unit price, VAT, amount…), in order, with their
    /// widths.
    #[rmcp::tool(annotations(
        title = "Get a document's line display",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_document_display(
        &self,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let display: Value = self
            .client()
            .call(Request::get(format!(
                "/v1/documents/{}/display",
                args.document_id
            )))
            .await?;
        Ok(Json(ApiObject::from_value(display)))
    }

    /// The payment schedule of a document: each milestone's amount, share,
    /// due date, and whether it is paid. Milestone ids feed the payment tools.
    #[rmcp::tool(annotations(
        title = "List a document's payment milestones",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_document_payment_milestones(
        &self,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let list: Value = self
            .client()
            .call(Request::get(format!(
                "/v1/documents/{}/paymentmilestones",
                args.document_id
            )))
            .await?;
        Ok(Json(Listing::from_value(list)))
    }

    /// How the payment-terms text of a document is composed (amounts,
    /// percentages, conditions, due dates shown or not), and the rendered text.
    #[rmcp::tool(annotations(
        title = "Get a document's payment terms display",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_payment_terms_display(
        &self,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let display: Value = self
            .client()
            .call(Request::get(format!(
                "/v1/documents/{}/paymenttermsdisplay",
                args.document_id
            )))
            .await?;
        Ok(Json(ApiObject::from_value(display)))
    }

    /// Change how a draft's payment-terms text is composed, or replace it with
    /// a custom `due_label`. Only the switches given change. Refused on a
    /// finalized document or a deposit document.
    #[rmcp::tool(annotations(
        title = "Set a document's payment terms display",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn set_payment_terms_display(
        &self,
        Parameters(args): Parameters<PaymentTermsArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let mut patch = to_api(&args.changes)?;
        let explicit_label = patch.get("dueLabel").is_some();
        let updated = self
            .modify(
                &format!("/v1/documents/{}/paymenttermsdisplay", args.document_id),
                std::mem::take(&mut patch),
                |merged, _| {
                    // The rendered text read back would override the switches.
                    if !explicit_label {
                        merged.remove("dueLabel");
                    }
                },
            )
            .await?;
        Ok(Json(ApiObject::from_value(updated)))
    }

    /// Finalize a draft: it gets its definitive number and is locked for good.
    /// Costs 8 credits for an invoice or credit note, 2 for other documents.
    /// A finalized invoice can never be deleted. Refuses to run without
    /// `confirm: true`; check the lines and the total first.
    #[rmcp::tool(annotations(
        title = "Finalize a document",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn finalize_document(
        &self,
        Parameters(args): Parameters<FinalizeArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let path = format!("/v1/documents/{}", args.document_id);
        if args.confirm != Some(true) {
            let summary = self.document_summary(&path).await;
            require_confirmation(
                None,
                &format!(
                    "document {}{summary} would be finalized: locked forever with a definitive number, at a cost of 8 credits for an invoice or credit note and 2 otherwise",
                    args.document_id
                ),
            )?;
        }
        let mut body = Map::new();
        if let Some(raw) = args.date.as_deref() {
            body.insert("date".into(), Value::String(dates::datetime(raw)?));
        }
        // The body schema refuses confirmFirstOfMonth: it travels in the query.
        let query = QueryBuilder::new()
            .opt("confirmFirstOfMonth", args.confirm_first_of_month)
            .build();
        let finalized: Value = self
            .client()
            .call(
                Request::post(format!("{path}/finalize"))
                    .query(query)
                    .json(Value::Object(body)),
            )
            .await?;
        Ok(Json(ApiObject::from_value(finalized)))
    }

    /// Turn a finalized quotation into a draft invoice: same customer, lines,
    /// prices, VAT and milestones, dated today. Returns the new invoice, with
    /// its own id; finalize it to give it a number. A quote is invoiced once.
    #[rmcp::tool(annotations(
        title = "Transform a quote into an invoice",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn transform_quote_to_invoice(
        &self,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let invoice: Value = self
            .client()
            .call(Request::post(format!(
                "/v1/documents/{}/transform-to-invoice",
                args.document_id
            )))
            .await?;
        Ok(Json(ApiObject::from_value(invoice)))
    }

    /// The document types of the company, with the id `create_document`
    /// takes: invoice, credit note, quote, delivery note… and custom ones.
    /// `isAccounting` marks those that cost 8 credits to finalize.
    #[rmcp::tool(annotations(
        title = "List document types",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_document_types(&self) -> Result<Json<Listing>, ToolError> {
        let list: Value = self
            .client()
            .call(Request::get("/v1/documenttypes"))
            .await?;
        Ok(Json(Listing::from_value(list)))
    }
}

impl HenrriServer {
    /// ` (Facture, 535.00 € incl. tax, customer PROBE MCP)` for a confirmation.
    async fn document_summary(&self, path: &str) -> String {
        let Ok(doc) = self.client().call::<Value>(Request::get(path)).await else {
            return String::new();
        };
        let mut parts = Vec::new();
        if let Some(kind) = doc.get("type").and_then(Value::as_str) {
            parts.push(kind.to_owned());
        }
        if let Some(total) = doc.get("priceAfterTax").and_then(Value::as_f64) {
            parts.push(format!("{total:.2} € incl. tax"));
        }
        if let Some(name) = doc.pointer("/customer/name").and_then(Value::as_str) {
            parts.push(format!("customer {name}"));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!(" ({})", parts.join(", "))
        }
    }
}

/// What a document `PUT` must not carry back.
fn adjust_document(merged: &mut Map<String, Value>, changes: &Map<String, Value>) {
    // Read-only projections of the ids; sent back they could contradict a change.
    for key in [
        "lines",
        "customer",
        "documentType",
        "label",
        "taxDetails",
        "display",
        "company",
    ] {
        merged.remove(key);
    }
    if changes.contains_key("customerId") && !changes.contains_key("customerAddress") {
        merged.remove("customerAddress");
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

    #[test]
    fn file_names_cannot_escape_the_download_directory() {
        assert_eq!(safe_file_name("../../etc/passwd", "d.pdf"), "passwd.pdf");
        assert_eq!(
            safe_file_name("Facture 2026-001.pdf", "d.pdf"),
            "Facture 2026-001.pdf"
        );
        assert_eq!(safe_file_name("..", "document-9.pdf"), "document-9.pdf");
        assert_eq!(safe_file_name("a:b|c", "d.pdf"), "a_b_c.pdf");
    }

    #[test]
    fn document_filters_refuse_state_and_finalized_together() {
        let both = DocumentFilters {
            state: Some(DocumentState::Finalized),
            finalized: Some(true),
            ..DocumentFilters::default()
        };
        assert!(both.apply(QueryBuilder::new()).is_err());
        let kinds = DocumentFilters {
            document_types: Some(vec![DocumentKind::Invoice, DocumentKind::CreditNote]),
            ..DocumentFilters::default()
        };
        assert_eq!(
            kinds.apply(QueryBuilder::new()).unwrap().build(),
            vec![
                ("documentTypes".to_owned(), "invoice".to_owned()),
                ("documentTypes".to_owned(), "creditNote".to_owned()),
            ]
        );
    }

    #[test]
    fn a_document_update_drops_projections_and_a_stale_address() {
        let mut merged = json!({
            "title": "t", "customerId": 2, "customer": {"id": 1}, "customerAddress": {"city": "A"},
            "lines": [], "documentType": {"id": 4}
        })
        .as_object()
        .cloned()
        .unwrap();
        let changes = json!({"customerId": 2}).as_object().cloned().unwrap();
        adjust_document(&mut merged, &changes);
        assert_eq!(
            Value::Object(merged),
            json!({"title": "t", "customerId": 2})
        );
    }

    #[test]
    fn a_human_date_becomes_a_datetime_in_the_body() {
        let mut body = json!({"date": "2026-09-15", "title": "x"});
        normalize_date(&mut body).unwrap();
        assert_eq!(body["date"], "2026-09-15T00:00:00");
    }
}
