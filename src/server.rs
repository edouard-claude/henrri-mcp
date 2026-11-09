//! The MCP server: state, capabilities, and the glue between the routers.

use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::router::{prompt::PromptRouter, tool::ToolRouter},
    model::{
        Implementation, InitializeResult, ListResourceTemplatesResult, ListResourcesResult,
        PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse,
        ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
};

use crate::{client::Henrri, config::Config, resources, webhooks::Inbox};

/// What the server tells a client about itself, once, at initialization.
///
/// This is the only place a model reads before deciding which tool to call, so
/// it carries what it cannot guess: ids versus numbers, drafts versus
/// finalized documents, euros, partial updates, and what costs credits.
pub const INSTRUCTIONS: &str = "\
Henrri: French invoicing software (devis, factures, avoirs, bons de livraison, \
acomptes, échéances, paiements). This server is unofficial and covers the \
whole Henrri v1 API.

Start with `whoami` to see the user and company this server acts as. Reference \
ids come from `list_document_types`, `list_document_line_types`, \
`list_payment_modes`, `list_item_categories` and `list_units` (also in the \
`henrri://reference` resource).

Vocabulary: a **document** is any commercial document; its kind (invoice, \
creditNote, quotation, deliveryNote, workOrder, depositInvoice…) comes from its \
document type. A document is created empty as a **draft**, gets **lines** \
(items, texts, subtotals), then is **finalized**: it receives its definitive \
number (`identity`) and is locked. Tools take the document **id**, not its \
number: find the id with `list_documents` and `search`. Its payment schedule is \
a list of **payment milestones** (échéances); paying one records a **payment**.

**Amounts are euros**, as decimals (`priceAfterTax: 535.0`). VAT rates are \
percentages (20 for 20 %). Dates accept `YYYY-MM-DD`, `DD/MM/YYYY`, `today`; \
list filters accept `period` (`this month`, `last month`, `2026-09`, `2026`, \
`last 30 days`), inclusive on both ends.

Update tools take only the fields that change and keep the rest: Henrri's own \
PUT erases what it is not sent, these tools read first and merge.

Irreversible tools refuse to run without `confirm: true`: `finalize_document` \
(locks the document; costs 8 credits for an invoice or credit note, 2 \
otherwise; a finalized invoice can never be deleted), `validate_quotation` (2 \
credits), the pay tools, `cancel_deposit` and every delete. Show the person \
what will happen and get their explicit agreement first. `get_api_usage` shows \
the credits left.

Failures come back as tool results with `isError` and Henrri's problem \
document: `errorCode` (ALREADY_FINALIZED, LINES_MISSING, QUOTA_EXCEEDED…), \
`detail`, field `errors`, and the `traceId` to give Henrri's support.";

/// One MCP server over one Henrri account.
pub struct HenrriServer {
    client: Arc<Henrri>,
    inbox: Arc<Inbox>,
    tool_router: ToolRouter<Self>,
    prompt_router: PromptRouter<Self>,
}

impl std::fmt::Debug for HenrriServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HenrriServer")
            .field("tools", &self.tool_router.list_all().len())
            .finish_non_exhaustive()
    }
}

impl HenrriServer {
    /// Build a server from a configuration, wiring only the enabled toolsets.
    pub fn new(config: Config) -> crate::error::Result<Self> {
        let client = Arc::new(Henrri::new(config)?);
        Ok(Self::with_client(client, Arc::new(Inbox::default())))
    }

    /// Build a server around an existing client and inbox, so HTTP sessions
    /// share one token and one webhook inbox.
    pub fn with_client(client: Arc<Henrri>, inbox: Arc<Inbox>) -> Self {
        let tool_router = crate::tools::router(client.config());
        Self {
            client,
            inbox,
            tool_router,
            prompt_router: crate::prompts::router(),
        }
    }

    /// The HTTP client.
    pub fn client(&self) -> &Arc<Henrri> {
        &self.client
    }

    /// The webhook deliveries received by this process.
    pub fn inbox(&self) -> &Arc<Inbox> {
        &self.inbox
    }

    /// The configuration in force.
    pub fn config(&self) -> &Config {
        self.client.config()
    }

    /// Every tool this server exposes, which is what `--list-tools` prints.
    pub fn tools(&self) -> Vec<Tool> {
        let mut tools = self.tool_router.list_all();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }
}

#[rmcp::tool_handler(router = self.tool_router)]
#[rmcp::prompt_handler(router = self.prompt_router)]
impl ServerHandler for HenrriServer {
    fn get_info(&self) -> ServerConfig {
        InitializeResult::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
        )
        .with_server_info(
            Implementation::new(crate::SERVER_NAME, crate::SERVER_VERSION)
                .with_title("Henrri (unofficial)")
                .with_description(
                    "Unofficial MCP server for the Henrri invoicing API: customers, quotes, invoices, payments, webhooks.",
                )
                .with_website_url("https://github.com/edouard-claude/henrri-mcp"),
        )
        .with_instructions(INSTRUCTIONS)
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListResourcesResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListResourcesResult::with_all_items(
            resources::catalogue(),
        )))
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListResourceTemplatesResult, McpError>> + Send + '_
    {
        std::future::ready(Ok(ListResourceTemplatesResult::with_all_items(
            resources::templates(),
        )))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResponse, McpError> {
        resources::read(self, &request.uri)
            .await
            .map(Into::into)
            .map_err(Into::into)
    }
}
