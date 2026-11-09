//! Customers and their contacts: 15 routes.
//!
//! A customer is `professional` (a company: 1 to 4 contacts, SIRET or BCE) or
//! `individual` (a person: exactly one contact). Contacts are managed through
//! their own routes; the customer update leaves them alone.

use rmcp::{Json, handler::server::wrapper::Parameters};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    client::Request,
    error::{Error, ToolError},
    server::HenrriServer,
    tools::{
        ApiObject, DateArgs, Done, Id, Listing, PageArgs, QueryBuilder, TableArgs,
        require_confirmation, to_api,
    },
};

/// Professional or individual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CustomerType {
    /// A company: 1 to 4 contacts, may carry a SIRET or BCE number.
    Professional,
    /// A person: exactly one contact, no company identifier.
    Individual,
}

/// How the company identifier reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CompanyIdentifierType {
    /// France: 14 digits. The default.
    Siret,
    /// Belgium: 10 digits starting with 0 or 1, dots allowed (0123.456.789).
    Bce,
}

/// A postal address. Only the fields given are changed on an update.
#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AddressInput {
    /// Number and street.
    pub address: Option<String>,
    /// City.
    pub city: Option<String>,
    /// Postal code. Henrri derives the city's spelling from it ("Paris 01"):
    /// change the two together, a city alone that does not match the code
    /// is blanked.
    pub post_code: Option<String>,
    /// Country name, e.g. France.
    pub country: Option<String>,
}

/// A contact person of a customer.
#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ContactInput {
    /// Civility, e.g. "M." or "Mme".
    pub title: Option<String>,
    /// First name.
    pub first_name: Option<String>,
    /// Last name. Required when creating a contact.
    pub last_name: Option<String>,
    /// Email address.
    pub email: Option<String>,
    /// Landline: digits, spaces and + - ( ) only.
    pub phone: Option<String>,
    /// Mobile: digits, spaces and + - ( ) only.
    pub mobile: Option<String>,
    /// Role or job title, e.g. "Directeur général".
    pub role: Option<String>,
    /// The primary contact. The first contact is primary when none is.
    pub is_primary: Option<bool>,
    /// Print this contact on documents.
    pub show_on_document: Option<bool>,
}

/// The customer fields both creation and update take.
#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct CustomerFields {
    /// Accounting reference of the customer.
    pub accounting_number: Option<String>,
    /// `siret` (default) or `bce`.
    pub company_identifier_type: Option<CompanyIdentifierType>,
    /// The SIRET (14 digits) or BCE (10 digits) number. Professionals only.
    pub siret: Option<String>,
    /// Trade name (nom commercial).
    pub trade_name: Option<String>,
    /// Legal form appended to the name: SAS, SARL, SA… Professionals only.
    pub trade_name_extension: Option<String>,
    /// Intra-community VAT identification code.
    pub ict: Option<String>,
    /// VAT number.
    pub vat_number: Option<String>,
    /// Billing address.
    pub address: Option<AddressInput>,
    /// Days after the due date before the first payment reminder.
    pub days_number_before_payment_reminder_level1: Option<i32>,
    /// Days after the due date before the second payment reminder.
    pub days_number_before_payment_reminder_level2: Option<i32>,
    /// Days after the due date before the third payment reminder.
    pub days_number_before_payment_reminder_level3: Option<i32>,
    /// Days after the due date before the fourth payment reminder.
    pub days_number_before_payment_reminder_level4: Option<i32>,
    /// Default discount on services, in percent.
    pub service_discount_percentage: Option<f64>,
    /// Default discount on products, in percent.
    pub product_discount_percentage: Option<f64>,
    /// Show an alert when this customer is used on a document.
    pub customer_type_alert_enabled: Option<bool>,
    /// This customer is also a supplier.
    pub is_supplier: Option<bool>,
    /// This customer is an advisor (accountant…).
    pub is_advisor: Option<bool>,
    /// Free comment. An empty string clears it.
    pub comment: Option<String>,
    /// Website URL.
    pub website: Option<String>,
}

/// Arguments of `create_customer`.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct CreateCustomerArgs {
    /// Display name: the company name for a professional (Henrri uppercases
    /// it), the last name for an individual.
    pub name: String,
    /// `professional` or `individual`.
    #[serde(rename = "type")]
    pub customer_type: CustomerType,
    /// The contacts: 1 to 4 for a professional, exactly 1 for an individual.
    /// Each needs a `last_name`.
    pub contacts: Vec<ContactInput>,
    /// Everything else.
    #[serde(flatten)]
    pub fields: CustomerFields,
}

/// What an update changes on a customer.
#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct CustomerChanges {
    /// New display name.
    pub name: Option<String>,
    /// New type.
    #[serde(rename = "type")]
    pub customer_type: Option<CustomerType>,
    /// Everything else.
    #[serde(flatten)]
    pub fields: CustomerFields,
}

/// Arguments of `update_customer`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateCustomerArgs {
    /// The customer to change.
    pub customer_id: Id,
    /// Only the fields to change; the others are kept as they are.
    #[serde(flatten)]
    pub changes: CustomerChanges,
}

/// Arguments of `list_customers`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListCustomersArgs {
    /// Paging, search and sort. `sort_by`: id, name or date.
    #[serde(flatten)]
    pub page: PageArgs,
    /// Creation-date range.
    #[serde(flatten)]
    pub dates: DateArgs,
}

/// Arguments of `list_customers_table`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct CustomersTableArgs {
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

/// Arguments of `list_best_customers`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct YearArgs {
    /// The year, e.g. 2026. Default: the current year.
    pub year: Option<i32>,
}

/// Arguments of `list_last_used_customers`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct LastUsedArgs {
    /// The customer types to include. Default: both.
    pub types: Option<Vec<CustomerType>>,
    /// How many customers. Default 30.
    pub limit: Option<u32>,
}

/// Arguments of the tools that act on one customer.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct CustomerArgs {
    /// The customer id.
    pub customer_id: Id,
}

/// Arguments of `delete_customer`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DeleteCustomerArgs {
    /// The customer to delete.
    pub customer_id: Id,
    /// Must be true: the customer and all its data are deleted for good.
    pub confirm: Option<bool>,
}

/// Arguments of `add_customer_contact`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct AddContactArgs {
    /// The customer.
    pub customer_id: Id,
    /// The contact. `last_name` is required.
    #[serde(flatten)]
    pub contact: ContactInput,
}

/// Arguments of the tools that read one contact.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ContactArgs {
    /// The customer.
    pub customer_id: Id,
    /// The contact, as `list_customer_contacts` shows it.
    pub contact_id: Id,
}

/// Arguments of `update_customer_contact`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateContactArgs {
    /// The customer.
    pub customer_id: Id,
    /// The contact to change.
    pub contact_id: Id,
    /// Only the fields to change; the others are kept as they are.
    #[serde(flatten)]
    pub changes: ContactInput,
}

/// Arguments of `delete_customer_contact`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct DeleteContactArgs {
    /// The customer.
    pub customer_id: Id,
    /// The contact to delete. A customer keeps at least one.
    pub contact_id: Id,
    /// Must be true: the contact is deleted for good.
    pub confirm: Option<bool>,
}

/// Refuse what Henrri would refuse, with a sentence that says how to fix it.
fn check_customer(args: &CreateCustomerArgs) -> crate::error::Result<()> {
    if args.name.trim().is_empty() {
        return Err(Error::invalid("name must not be empty"));
    }
    let contacts = args.contacts.len();
    match args.customer_type {
        CustomerType::Professional if !(1..=4).contains(&contacts) => {
            return Err(Error::invalid(format!(
                "a professional customer has 1 to 4 contacts, got {contacts}"
            )));
        }
        CustomerType::Individual if contacts != 1 => {
            return Err(Error::invalid(format!(
                "an individual customer has exactly 1 contact, got {contacts}"
            )));
        }
        CustomerType::Individual
            if args.fields.siret.is_some() || args.fields.trade_name_extension.is_some() =>
        {
            return Err(Error::invalid(
                "an individual customer has no siret nor trade_name_extension",
            ));
        }
        _ => {}
    }
    if let Some(i) = args
        .contacts
        .iter()
        .position(|c| c.last_name.as_deref().is_none_or(|n| n.trim().is_empty()))
    {
        return Err(Error::invalid(format!(
            "contact {} has no last_name, which Henrri requires",
            i + 1
        )));
    }
    check_identifier(&args.fields)
}

fn check_identifier(fields: &CustomerFields) -> crate::error::Result<()> {
    let Some(raw) = fields.siret.as_deref().filter(|s| !s.trim().is_empty()) else {
        return Ok(());
    };
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    let foreign = raw
        .chars()
        .any(|c| !(c.is_ascii_digit() || c == ' ' || c == '.'));
    match fields
        .company_identifier_type
        .unwrap_or(CompanyIdentifierType::Siret)
    {
        CompanyIdentifierType::Siret if foreign || digits.len() != 14 => {
            Err(Error::invalid(format!(
                "a SIRET has exactly 14 digits, got {raw:?}; for a Belgian BCE number set company_identifier_type: bce"
            )))
        }
        CompanyIdentifierType::Bce
            if foreign || digits.len() != 10 || !digits.starts_with(['0', '1']) =>
        {
            Err(Error::invalid(format!(
                "a BCE number has 10 digits starting with 0 or 1 (0123.456.789), got {raw:?}"
            )))
        }
        _ => Ok(()),
    }
}

#[allow(missing_docs, reason = "the router function is generated by the macro")]
#[rmcp::tool_router(router = customers_router, vis = "pub")]
impl HenrriServer {
    /// List customers, page by page, with free-text search (name, contact,
    /// city…), a creation-date range and sorting (`sort_by`: id, name, date).
    #[rmcp::tool(annotations(
        title = "List customers",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_customers(
        &self,
        Parameters(args): Parameters<ListCustomersArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let query = QueryBuilder::new()
            .paged(&args.page)
            .dated(&args.dates)?
            .build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/customers").query(query))
            .await?;
        Ok(Json(Listing::from_value(page)))
    }

    /// Create a customer with its contacts. A professional needs 1 to 4
    /// contacts, an individual exactly one; every contact needs a last name.
    /// Returns the customer with its id.
    #[rmcp::tool(annotations(
        title = "Create a customer",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn create_customer(
        &self,
        Parameters(args): Parameters<CreateCustomerArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        check_customer(&args)?;
        let created: Value = self
            .client()
            .call(Request::post("/v1/customers").json(to_api(&args)?))
            .await?;
        Ok(Json(ApiObject::from_value(created)))
    }

    /// The 30 customers with the highest sales over a year (default: this
    /// year), best first.
    #[rmcp::tool(annotations(
        title = "List best customers",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_best_customers(
        &self,
        Parameters(args): Parameters<YearArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let query = QueryBuilder::new().opt("year", args.year).build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/customers/best-sales").query(query))
            .await?;
        Ok(Json(Listing::from_value(page)))
    }

    /// The customers used most recently on documents (default 30), of the
    /// given types: the quickest way to find "the customer from last week".
    #[rmcp::tool(annotations(
        title = "List recently used customers",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_last_used_customers(
        &self,
        Parameters(args): Parameters<LastUsedArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let types = args
            .types
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| vec![CustomerType::Professional, CustomerType::Individual]);
        let query = QueryBuilder::new()
            .list("Types", Some(&types))
            .opt("Limit", args.limit)
            .build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/customers/last-used").query(query))
            .await?;
        Ok(Json(Listing::from_value(page)))
    }

    /// Customers as Henrri's own customer list shows them: the columns the
    /// user chose in Henrri, already formatted, one object per row, with an
    /// optional totals row. Use `list_customers` for the raw fields.
    #[rmcp::tool(annotations(
        title = "List customers as a table",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_customers_table(
        &self,
        Parameters(args): Parameters<CustomersTableArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let query = QueryBuilder::new()
            .paged(&args.page)
            .dated(&args.dates)?
            .table(&args.table)
            .build();
        let page: Value = self
            .client()
            .call(Request::get("/v1/customers/with-selected-fields").query(query))
            .await?;
        Ok(Json(Listing::from_table(page)))
    }

    /// Read one customer in full: identity, identifiers, address, contacts,
    /// reminder settings, discounts. A customer copied onto a finalized
    /// document (`parentCustomerId` set) is a frozen copy: read the parent.
    #[rmcp::tool(annotations(
        title = "Get a customer",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_customer(
        &self,
        Parameters(args): Parameters<CustomerArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let customer: Value = self
            .client()
            .call(Request::get(format!("/v1/customers/{}", args.customer_id)))
            .await?;
        Ok(Json(ApiObject::from_value(customer)))
    }

    /// Change some fields of a customer: pass only what changes, the rest is
    /// kept (Henrri's PUT would erase what is left out, so this tool reads the
    /// customer first and writes the whole back). Contacts have their own tools.
    #[rmcp::tool(annotations(
        title = "Update a customer",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn update_customer(
        &self,
        Parameters(args): Parameters<UpdateCustomerArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        check_identifier(&args.changes.fields)?;
        let updated = self
            .modify(
                &format!("/v1/customers/{}", args.customer_id),
                to_api(&args.changes)?,
                // A customer PUT carrying contacts is refused (PUT_WITH_CUSTOMER).
                |merged, _| {
                    merged.remove("contacts");
                },
            )
            .await?;
        Ok(Json(ApiObject::from_value(updated)))
    }

    /// Delete a customer and all its data, for good. Refuses to run without
    /// `confirm: true`. Henrri refuses it for a customer used on documents.
    #[rmcp::tool(annotations(
        title = "Delete a customer",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn delete_customer(
        &self,
        Parameters(args): Parameters<DeleteCustomerArgs>,
    ) -> Result<Json<Done>, ToolError> {
        let path = format!("/v1/customers/{}", args.customer_id);
        if args.confirm != Some(true) {
            let name = self.describe(&path, "name").await;
            require_confirmation(
                None,
                &format!(
                    "customer {}{name} and all its data would be deleted",
                    args.customer_id
                ),
            )?;
        }
        let response: Value = self.client().call(Request::delete(path)).await?;
        Ok(Json(Done::new(
            format!("customer {} deleted", args.customer_id),
            response,
        )))
    }

    /// Read the billing address of a customer: street, city, postal code,
    /// country.
    #[rmcp::tool(annotations(
        title = "Get a customer's address",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_customer_address(
        &self,
        Parameters(args): Parameters<CustomerArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let address: Value = self
            .client()
            .call(Request::get(format!(
                "/v1/customers/{}/address",
                args.customer_id
            )))
            .await?;
        Ok(Json(ApiObject::from_value(address)))
    }

    /// Every payment milestone of a customer's finalized documents: what they
    /// owe, when, on which document, whether it is paid. A finalized document
    /// points at a frozen copy of the customer (its `customerId`, whose
    /// `parentCustomerId` is the customer): pass that copy's id.
    #[rmcp::tool(annotations(
        title = "List a customer's payment milestones",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_customer_payment_milestones(
        &self,
        Parameters(args): Parameters<CustomerArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let list: Value = self
            .client()
            .call(Request::get(format!(
                "/v1/customers/{}/paymentmilestones",
                args.customer_id
            )))
            .await?;
        Ok(Json(Listing::from_value(list)))
    }

    /// Add a contact to a customer. A professional has at most 4 contacts, an
    /// individual exactly one. `last_name` is required.
    #[rmcp::tool(annotations(
        title = "Add a contact",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn add_customer_contact(
        &self,
        Parameters(args): Parameters<AddContactArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        if args
            .contact
            .last_name
            .as_deref()
            .is_none_or(|n| n.trim().is_empty())
        {
            return Err(Error::invalid("last_name is required").into());
        }
        let created: Value = self
            .client()
            .call(
                Request::post(format!("/v1/customers/{}/contacts", args.customer_id))
                    .json(to_api(&args.contact)?),
            )
            .await?;
        Ok(Json(ApiObject::from_value(created)))
    }

    /// List the contacts of a customer, with their ids.
    #[rmcp::tool(annotations(
        title = "List a customer's contacts",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_customer_contacts(
        &self,
        Parameters(args): Parameters<CustomerArgs>,
    ) -> Result<Json<Listing>, ToolError> {
        let list: Value = self
            .client()
            .call(Request::get(format!(
                "/v1/customers/{}/contacts",
                args.customer_id
            )))
            .await?;
        Ok(Json(Listing::from_value(list)))
    }

    /// Read one contact of a customer: civility, names, email, phones, role,
    /// primary flag.
    #[rmcp::tool(annotations(
        title = "Get a contact",
        read_only_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_customer_contact(
        &self,
        Parameters(args): Parameters<ContactArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let contact: Value = self
            .client()
            .call(Request::get(format!(
                "/v1/customers/{}/contacts/{}",
                args.customer_id, args.contact_id
            )))
            .await?;
        Ok(Json(ApiObject::from_value(contact)))
    }

    /// Change some fields of a contact: pass only what changes, the rest is
    /// kept.
    #[rmcp::tool(annotations(
        title = "Update a contact",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn update_customer_contact(
        &self,
        Parameters(args): Parameters<UpdateContactArgs>,
    ) -> Result<Json<ApiObject>, ToolError> {
        let updated = self
            .modify(
                &format!(
                    "/v1/customers/{}/contacts/{}",
                    args.customer_id, args.contact_id
                ),
                to_api(&args.changes)?,
                |_, _| {},
            )
            .await?;
        Ok(Json(ApiObject::from_value(updated)))
    }

    /// Delete a contact of a customer, for good. A customer keeps at least one
    /// contact. Refuses to run without `confirm: true`.
    #[rmcp::tool(annotations(
        title = "Delete a contact",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn delete_customer_contact(
        &self,
        Parameters(args): Parameters<DeleteContactArgs>,
    ) -> Result<Json<Done>, ToolError> {
        let path = format!(
            "/v1/customers/{}/contacts/{}",
            args.customer_id, args.contact_id
        );
        if args.confirm != Some(true) {
            let name = self.describe(&path, "lastName").await;
            require_confirmation(
                None,
                &format!(
                    "contact {}{name} of customer {} would be deleted",
                    args.contact_id, args.customer_id
                ),
            )?;
        }
        let response: Value = self.client().call(Request::delete(path)).await?;
        Ok(Json(Done::new(
            format!(
                "contact {} of customer {} deleted",
                args.contact_id, args.customer_id
            ),
            response,
        )))
    }
}

impl HenrriServer {
    /// ` (NAME)` for a confirmation message, or nothing when the read fails:
    /// the refusal must not depend on the read succeeding.
    pub(crate) async fn describe(&self, path: &str, field: &str) -> String {
        self.client()
            .call::<Value>(Request::get(path))
            .await
            .ok()
            .and_then(|v| v.get(field).and_then(Value::as_str).map(str::to_owned))
            .filter(|s| !s.is_empty())
            .map(|s| format!(" ({s})"))
            .unwrap_or_default()
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

    fn create(value: Value) -> CreateCustomerArgs {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn the_creation_body_is_camel_case_with_type_and_contacts() {
        let args = create(json!({
            "name": "Atelier Martin",
            "type": "professional",
            "contacts": [{"last_name": "Martin", "first_name": "Ada", "is_primary": true}],
            "siret": "12345678901234",
            "address": {"post_code": "68100", "city": "Mulhouse"},
            "days_number_before_payment_reminder_level1": 15
        }));
        check_customer(&args).unwrap();
        let body = to_api(&args).unwrap();
        assert_eq!(body["type"], "professional");
        assert_eq!(body["contacts"][0]["lastName"], "Martin");
        assert_eq!(body["contacts"][0]["isPrimary"], true);
        assert_eq!(body["address"]["postCode"], "68100");
        assert_eq!(body["daysNumberBeforePaymentReminderLevel1"], 15);
        assert!(body.get("comment").is_none(), "absent fields stay absent");
    }

    #[test]
    fn contact_counts_follow_the_customer_type() {
        let no_contact = create(json!({"name": "X", "type": "professional", "contacts": []}));
        assert!(check_customer(&no_contact).is_err());
        let two = create(json!({
            "name": "X", "type": "individual",
            "contacts": [{"last_name": "A"}, {"last_name": "B"}]
        }));
        assert!(check_customer(&two).is_err());
        let nameless =
            create(json!({"name": "X", "type": "individual", "contacts": [{"first_name": "A"}]}));
        assert!(
            check_customer(&nameless)
                .unwrap_err()
                .to_string()
                .contains("last_name")
        );
    }

    #[test]
    fn company_identifiers_are_checked_by_type() {
        let siret = |s: &str, t: Option<CompanyIdentifierType>| CustomerFields {
            siret: Some(s.into()),
            company_identifier_type: t,
            ..CustomerFields::default()
        };
        assert!(check_identifier(&siret("123 456 789 01234", None)).is_ok());
        assert!(check_identifier(&siret("123456789", None)).is_err());
        assert!(check_identifier(&siret("0123.456.789", Some(CompanyIdentifierType::Bce))).is_ok());
        assert!(
            check_identifier(&siret("2123.456.789", Some(CompanyIdentifierType::Bce))).is_err()
        );
    }
}
