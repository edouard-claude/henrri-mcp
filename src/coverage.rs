//! Which tool covers which operation of the Henrri v1 specification.
//!
//! The table is the contract: a test checks that it lists exactly the
//! operations of `spec/operations.tsv` (itself checked against the live
//! specifications in CI) and that every tool it names exists. An operation
//! added upstream fails the build until a tool covers it.

/// How an operation is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// A tool of that name.
    Tool(&'static str),
    /// The client itself, transparently: the authentication routes.
    Client,
}

/// One operation: method, path, `operationId`, and what covers it.
pub type Operation = (&'static str, &'static str, &'static str, Via);

/// Every operation of the specification, in `spec/operations.tsv` order.
pub const OPERATIONS: [Operation; 87] = [
    (
        "GET",
        "/v1/companies/{id}",
        "Company.Get",
        Via::Tool("get_company"),
    ),
    (
        "GET",
        "/v1/companies/{id}/address",
        "Company.GetAddress",
        Via::Tool("get_company_address"),
    ),
    (
        "GET",
        "/v1/customers",
        "Customer.GetAll",
        Via::Tool("list_customers"),
    ),
    (
        "POST",
        "/v1/customers",
        "Customer.Add",
        Via::Tool("create_customer"),
    ),
    (
        "GET",
        "/v1/customers/best-sales",
        "Customer.GetBestSales",
        Via::Tool("list_best_customers"),
    ),
    (
        "GET",
        "/v1/customers/last-used",
        "Customer.GetLastUsed",
        Via::Tool("list_last_used_customers"),
    ),
    (
        "GET",
        "/v1/customers/with-selected-fields",
        "Customer.GetAllWithSelectedFields",
        Via::Tool("list_customers_table"),
    ),
    (
        "DELETE",
        "/v1/customers/{id}",
        "Customer.Delete",
        Via::Tool("delete_customer"),
    ),
    (
        "GET",
        "/v1/customers/{id}",
        "Customer.Get",
        Via::Tool("get_customer"),
    ),
    (
        "PUT",
        "/v1/customers/{id}",
        "Customer.Modify",
        Via::Tool("update_customer"),
    ),
    (
        "GET",
        "/v1/customers/{id}/address",
        "Customer.GetAddress",
        Via::Tool("get_customer_address"),
    ),
    (
        "GET",
        "/v1/customers/{id}/contacts",
        "Customer.GetContacts",
        Via::Tool("list_customer_contacts"),
    ),
    (
        "POST",
        "/v1/customers/{id}/contacts",
        "Customer.AddContact",
        Via::Tool("add_customer_contact"),
    ),
    (
        "DELETE",
        "/v1/customers/{id}/contacts/{contactId}",
        "Customer.DeleteContact",
        Via::Tool("delete_customer_contact"),
    ),
    (
        "GET",
        "/v1/customers/{id}/contacts/{contactId}",
        "Customer.GetContact",
        Via::Tool("get_customer_contact"),
    ),
    (
        "PUT",
        "/v1/customers/{id}/contacts/{contactId}",
        "Customer.ModifyContact",
        Via::Tool("update_customer_contact"),
    ),
    (
        "GET",
        "/v1/customers/{id}/paymentmilestones",
        "Customer.GetPaymentMilestones",
        Via::Tool("list_customer_payment_milestones"),
    ),
    (
        "GET",
        "/v1/documentlinetypes",
        "DocumentLineType.GetAll",
        Via::Tool("list_document_line_types"),
    ),
    (
        "GET",
        "/v1/documents",
        "Document.GetAll",
        Via::Tool("list_documents"),
    ),
    (
        "POST",
        "/v1/documents",
        "Document.Add",
        Via::Tool("create_document"),
    ),
    (
        "GET",
        "/v1/documents/next-quote-batch",
        "Document.GetNextQuoteBatch",
        Via::Tool("next_quote_batch"),
    ),
    (
        "GET",
        "/v1/documents/with-selected-fields",
        "Document.GetAllWithSelectedFields",
        Via::Tool("list_documents_table"),
    ),
    (
        "GET",
        "/v1/documents/{documentId}/lines",
        "Line.GetAll",
        Via::Tool("list_document_lines"),
    ),
    (
        "POST",
        "/v1/documents/{documentId}/lines",
        "Line.Add",
        Via::Tool("add_document_line"),
    ),
    (
        "DELETE",
        "/v1/documents/{documentId}/lines/{id}",
        "Line.Delete",
        Via::Tool("delete_document_line"),
    ),
    (
        "GET",
        "/v1/documents/{documentId}/lines/{id}",
        "Line.Get",
        Via::Tool("get_document_line"),
    ),
    (
        "PUT",
        "/v1/documents/{documentId}/lines/{id}",
        "Line.Modify",
        Via::Tool("update_document_line"),
    ),
    (
        "POST",
        "/v1/documents/{documentId}/lines/{id}/move",
        "Line.Move",
        Via::Tool("move_document_line"),
    ),
    (
        "GET",
        "/v1/documents/{documentId}/paymentmilestones",
        "Document.GetPaymentMilestones",
        Via::Tool("list_document_payment_milestones"),
    ),
    (
        "DELETE",
        "/v1/documents/{id}",
        "Document.Delete",
        Via::Tool("delete_document"),
    ),
    (
        "GET",
        "/v1/documents/{id}",
        "Document.Get",
        Via::Tool("get_document"),
    ),
    (
        "PUT",
        "/v1/documents/{id}",
        "Document.Modify",
        Via::Tool("update_document"),
    ),
    (
        "GET",
        "/v1/documents/{id}/all-included",
        "Document.GetWithAllIncluded",
        Via::Tool("get_document_all_included"),
    ),
    (
        "GET",
        "/v1/documents/{id}/display",
        "Document.GetDisplay",
        Via::Tool("get_document_display"),
    ),
    (
        "POST",
        "/v1/documents/{id}/finalize",
        "Document.Finalize",
        Via::Tool("finalize_document"),
    ),
    (
        "GET",
        "/v1/documents/{id}/paymenttermsdisplay",
        "Document.GetPaymentTermsDisplay",
        Via::Tool("get_payment_terms_display"),
    ),
    (
        "PUT",
        "/v1/documents/{id}/paymenttermsdisplay",
        "Document.SetPaymentTermsDisplay",
        Via::Tool("set_payment_terms_display"),
    ),
    (
        "GET",
        "/v1/documents/{id}/pdf",
        "Document.GeneratePdf",
        Via::Tool("get_document_pdf"),
    ),
    (
        "GET",
        "/v1/documents/{id}/pdf/files/{guid}",
        "Document.DownloadPdfFile",
        Via::Tool("download_document_pdf_file"),
    ),
    (
        "POST",
        "/v1/documents/{id}/pdf/url",
        "Document.GeneratePdfUrl",
        Via::Tool("create_document_pdf_url"),
    ),
    (
        "GET",
        "/v1/documents/{id}/tax-details",
        "Document.GetTaxDetails",
        Via::Tool("get_document_tax_details"),
    ),
    (
        "POST",
        "/v1/documents/{id}/transform-to-invoice",
        "Document.TransformToInvoice",
        Via::Tool("transform_quote_to_invoice"),
    ),
    (
        "POST",
        "/v1/documents/{id}/validate",
        "Document.Validate",
        Via::Tool("validate_quotation"),
    ),
    (
        "GET",
        "/v1/documents/{id}/with-all",
        "Document.GetWithAll",
        Via::Tool("get_document_with_all"),
    ),
    (
        "GET",
        "/v1/documenttypes",
        "DocumentType.GetAll",
        Via::Tool("list_document_types"),
    ),
    (
        "GET",
        "/v1/itemcategories",
        "ItemCategory.GetAll",
        Via::Tool("list_item_categories"),
    ),
    ("GET", "/v1/items", "Item.GetAll", Via::Tool("list_items")),
    ("POST", "/v1/items", "Item.Add", Via::Tool("create_item")),
    (
        "GET",
        "/v1/items/best-sales",
        "Item.GetBestSales",
        Via::Tool("list_best_selling_items"),
    ),
    (
        "GET",
        "/v1/items/most-used",
        "Item.GetMostUsed",
        Via::Tool("list_most_used_items"),
    ),
    (
        "GET",
        "/v1/items/with-selected-fields",
        "Item.GetAllWithSelectedFields",
        Via::Tool("list_items_table"),
    ),
    (
        "DELETE",
        "/v1/items/{id}",
        "Item.Delete",
        Via::Tool("delete_item"),
    ),
    ("GET", "/v1/items/{id}", "Item.Get", Via::Tool("get_item")),
    (
        "PUT",
        "/v1/items/{id}",
        "Item.Modify",
        Via::Tool("update_item"),
    ),
    (
        "GET",
        "/v1/paymentmilestones",
        "PaymentMilestone.GetAll",
        Via::Tool("list_payment_milestones"),
    ),
    (
        "POST",
        "/v1/paymentmilestones",
        "PaymentMilestone.Add",
        Via::Tool("create_payment_milestone"),
    ),
    (
        "POST",
        "/v1/paymentmilestones/pay",
        "PaymentMilestone.PayMultiple",
        Via::Tool("pay_payment_milestones"),
    ),
    (
        "GET",
        "/v1/paymentmilestones/with-selected-fields",
        "PaymentMilestone.GetAllWithSelectedFields",
        Via::Tool("list_unpaid_milestones_table"),
    ),
    (
        "DELETE",
        "/v1/paymentmilestones/{id}",
        "PaymentMilestone.Delete",
        Via::Tool("delete_payment_milestone"),
    ),
    (
        "GET",
        "/v1/paymentmilestones/{id}",
        "PaymentMilestone.Get",
        Via::Tool("get_payment_milestone"),
    ),
    (
        "PUT",
        "/v1/paymentmilestones/{id}",
        "PaymentMilestone.Modify",
        Via::Tool("update_payment_milestone"),
    ),
    (
        "DELETE",
        "/v1/paymentmilestones/{id}/deposit",
        "PaymentMilestone.CancelDeposit",
        Via::Tool("cancel_deposit"),
    ),
    (
        "POST",
        "/v1/paymentmilestones/{id}/deposit-request",
        "PaymentMilestone.CreateDepositRequest",
        Via::Tool("create_deposit_request"),
    ),
    (
        "POST",
        "/v1/paymentmilestones/{id}/deposit-settlement",
        "PaymentMilestone.CreateDepositSettlement",
        Via::Tool("create_deposit_settlement"),
    ),
    (
        "POST",
        "/v1/paymentmilestones/{id}/pay",
        "PaymentMilestone.Pay",
        Via::Tool("pay_payment_milestone"),
    ),
    (
        "GET",
        "/v1/paymentmilestones/{id}/payment",
        "PaymentMilestone.GetPayment",
        Via::Tool("get_milestone_payment"),
    ),
    (
        "GET",
        "/v1/paymentmodes",
        "PaymentMode.GetAll",
        Via::Tool("list_payment_modes"),
    ),
    (
        "GET",
        "/v1/payments",
        "Payment.GetAll",
        Via::Tool("list_payments"),
    ),
    (
        "GET",
        "/v1/payments/{id}",
        "Payment.Get",
        Via::Tool("get_payment"),
    ),
    (
        "GET",
        "/v1/payments/{id}/milestones",
        "Payment.GetMilestones",
        Via::Tool("list_payment_settled_milestones"),
    ),
    (
        "GET",
        "/v1/revenues/{year}",
        "Revenue.Yearly",
        Via::Tool("get_yearly_revenue"),
    ),
    (
        "GET",
        "/v1/revenues/{year}/months",
        "Revenue.Monthly",
        Via::Tool("get_monthly_revenue"),
    ),
    (
        "GET",
        "/v1/secures/hello-world",
        "Secure_HelloWorld",
        Via::Tool("ping"),
    ),
    (
        "GET",
        "/v1/textitems",
        "TextItem.GetAll",
        Via::Tool("list_text_items"),
    ),
    (
        "POST",
        "/v1/textitems",
        "TextItem.Add",
        Via::Tool("create_text_item"),
    ),
    (
        "DELETE",
        "/v1/textitems/{id}",
        "TextItem.Delete",
        Via::Tool("delete_text_item"),
    ),
    (
        "GET",
        "/v1/textitems/{id}",
        "TextItem.Get",
        Via::Tool("get_text_item"),
    ),
    (
        "PUT",
        "/v1/textitems/{id}",
        "TextItem.Modify",
        Via::Tool("update_text_item"),
    ),
    ("GET", "/v1/units", "Unit.GetAll", Via::Tool("list_units")),
    ("POST", "/v1/units", "Unit.Add", Via::Tool("create_unit")),
    ("GET", "/v1/units/{id}", "Unit.Get", Via::Tool("get_unit")),
    (
        "POST",
        "/v1/users/authenticate",
        "User.Authenticate.Json",
        Via::Client,
    ),
    (
        "GET",
        "/v1/users/companies",
        "UserCompany.Get",
        Via::Tool("get_my_company"),
    ),
    (
        "GET",
        "/v1/users/me",
        "UserInformation.Get",
        Via::Tool("whoami"),
    ),
    (
        "POST",
        "/v1/users/refresh-token",
        "User.RefreshToken",
        Via::Client,
    ),
    ("GET", "/v1/users/{id}", "User.Get", Via::Tool("get_user")),
    (
        "GET",
        "/v1/users/{id}/address",
        "User.GetAddress",
        Via::Tool("get_user_address"),
    ),
];

/// The tools that cover no single route, and why they exist.
pub const EXTRA_TOOLS: [(&str, &str); 4] = [
    (
        "get_api_usage",
        "credit counters travel only in X-Credits-* headers",
    ),
    (
        "list_webhook_event_types",
        "the specification's webhooks section",
    ),
    (
        "verify_webhook_signature",
        "the specification's webhook signature",
    ),
    (
        "list_received_webhook_events",
        "deliveries received at /webhooks",
    ),
];

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot fail loudly is not a test"
    )]
    use std::collections::BTreeSet;

    use super::*;

    const SPEC: &str = include_str!("../spec/operations.tsv");

    #[test]
    fn the_table_is_exactly_the_specification() {
        let spec: BTreeSet<(String, String, String)> = SPEC
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .map(|l| {
                let mut parts = l.split('\t');
                (
                    parts.next().unwrap().to_owned(),
                    parts.next().unwrap().to_owned(),
                    parts.next().unwrap().to_owned(),
                )
            })
            .collect();
        let table: BTreeSet<(String, String, String)> = OPERATIONS
            .iter()
            .map(|(m, p, id, _)| ((*m).to_owned(), (*p).to_owned(), (*id).to_owned()))
            .collect();
        let missing: Vec<_> = spec.difference(&table).collect();
        let extra: Vec<_> = table.difference(&spec).collect();
        assert!(missing.is_empty(), "operations without a tool: {missing:?}");
        assert!(
            extra.is_empty(),
            "operations no longer in the spec: {extra:?}"
        );
    }

    #[test]
    fn every_tool_is_used_once() {
        let mut seen = BTreeSet::new();
        for (_, _, id, via) in OPERATIONS {
            if let Via::Tool(name) = via {
                assert!(
                    seen.insert(name),
                    "{name} covers two operations, {id} among them"
                );
            }
        }
        for (name, _) in EXTRA_TOOLS {
            assert!(seen.insert(name), "{name} is both extra and covering");
        }
        assert_eq!(seen.len(), 89);
    }
}
