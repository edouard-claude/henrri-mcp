<div align="center">

# mcp-henrri

**Your Henrri invoicing, answered by asking.**

A single Rust binary that puts the whole [Henrri](https://www.henrri.com) v1 API —
customers, quotes, invoices, credit notes, payment schedules, deposits, payments, webhooks —
in front of any MCP client.

[![Unofficial](https://img.shields.io/badge/status-unofficial-orange)](#disclaimer)
[![Rust](https://img.shields.io/badge/Rust-1.88%2B-CE422B?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![MCP](https://img.shields.io/badge/MCP-2025--11--25-8A63D2)](https://modelcontextprotocol.io)
[![Henrri API](https://img.shields.io/badge/Henrri%20API%20v1-87%2F87%20operations-1f6feb)](https://api.henrri.io/scalar/)
[![Transports](https://img.shields.io/badge/transports-stdio%20%2B%20Streamable%20HTTP-0b7285)](#run-it-over-http)
[![CI](https://github.com/edouard-claude/henrri-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/edouard-claude/henrri-mcp/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/edouard-claude/henrri-mcp?color=1f6feb)](https://github.com/edouard-claude/henrri-mcp/releases/latest)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

🇫🇷 *Serveur MCP non officiel pour Henrri : clients, devis, factures, avoirs, échéances,
acomptes et paiements, pilotés en langage naturel depuis Claude, Cursor ou tout client MCP.*

</div>

> [!IMPORTANT]
> **This is not an official Henrri product.** It is an independent, open-source project,
> not made, endorsed or supported by Henrri. It covers the published Henrri v1 specification
> operation by operation, as faithfully as the API allows, and documents where the API
> behaves differently from its documentation. See [Disclaimer](#disclaimer).

---

Ask your assistant *"invoice Atelier Martin for three days of development at 600 € excl.
tax"*, *"who still owes me money?"*, *"turn the signed quote D-26-11-3 into an invoice"* —
and get it done in the same Henrri your accountant reads.

```console
> who still owes me money?

  list_payment_milestones   is_overdue = true · 7 milestones
                            ATELIER MARTIN    I-26-10-12   1 440,00 €   23 days late
                            LOVELACE          I-26-10-17     864,00 €    9 days late
                            …
                            overdue total     4 812,40 €

> invoice Atelier Martin for 3 days of development at 600 € excl. tax

  create_document           draft invoice 9632 for ATELIER MARTIN
  add_document_line         3 × Journée de développement · 600,00 € · VAT 20 %
  get_document_with_all     1 800,00 € excl. tax · 360,00 € VAT · 2 160,00 € incl. tax
                            finalize it? It locks the invoice for good and costs 8 credits.
```

## Why

Henrri's API is a clean REST surface, but handed raw to a language model it goes wrong in
predictable ways: a `PUT` erases every field it is not sent, a document is a draft until a
call that can never be undone, finalizing costs credits, the monthly quota and the rate limiter
both answer `429`, and a model cannot tell a document's id from its number.

- **The whole API, nothing hidden.** All 87 operations of the v1 specification: 85 routes, one
  tool each, and the two authentication routes driven by the client itself. A test checks the
  tool table against [`spec/operations.tsv`](spec/operations.tsv), and CI checks that file
  against the live production *and* sandbox specifications every day.
- **Updates that update.** Every `update_*` tool takes only what changes, reads the resource,
  merges, and writes back what Henrri accepts — no more customers losing their comment because
  their website changed.
- **Fail-closed where it cannot be undone.** `finalize_document`, `validate_quotation`, the pay
  tools, `cancel_deposit` and every delete refuse to run without `confirm: true`, and the refusal
  states what would happen: the document, its total, its customer, the credits.
  `HENRRI_READ_ONLY=1` withholds every write tool.
- **Errors a model can act on.** Failures come back as tool results with `isError: true` and
  Henrri's RFC 9457 problem document: the stable `errorCode` (`ALREADY_FINALIZED`,
  `LINES_MISSING`, `QUOTA_EXCEEDED`…), the `detail`, the invalid fields, and the `traceId` for
  Henrri's support, plus a hint when the fix is not obvious.
- **The API's manners, handled once.** Ten-minute tokens renewed ahead of expiry; the rate
  limiter, `503` and busy finalizations retried; the credit quota never retried; a `POST` never
  resent after it may have run; list filters sent as repeated keys; periods such as
  `last month` turned into Henrri's inclusive dates.
- **Webhooks, verified.** Over HTTP the server receives Henrri's deliveries at `/webhooks`,
  checks their HMAC-SHA256 signature in constant time, refuses replays, and lets the model read
  what happened.
- **Written for a model to read.** Structured output with schemas on all 89 tools, annotations
  on every one, four prompts that stop and ask before anything irreversible, and server
  instructions that say up front: ids not numbers, euros, drafts versus finalized.

## Install

<details open>
<summary><b>Prebuilt binary (macOS, Linux, Windows)</b></summary>

Download the archive for your platform from the
[latest release](https://github.com/edouard-claude/henrri-mcp/releases/latest),
check it against its `.sha256`, and put `mcp-henrri` on your `PATH`:

```bash
# macOS on Apple silicon; swap the target for x86_64-apple-darwin,
# x86_64-unknown-linux-gnu or aarch64-unknown-linux-gnu
V=v0.1.0 T=aarch64-apple-darwin
curl -LO https://github.com/edouard-claude/henrri-mcp/releases/download/$V/mcp-henrri-$V-$T.tar.gz
tar xzf mcp-henrri-$V-$T.tar.gz
sudo mv mcp-henrri-$V-$T/mcp-henrri /usr/local/bin/
```

Windows: take the `x86_64-pc-windows-msvc.zip` archive.
</details>

<details>
<summary><b>From source (any platform with Rust 1.88+)</b></summary>

```bash
git clone https://github.com/edouard-claude/henrri-mcp
cd henrri-mcp
cargo build --release
# the binary is at target/release/mcp-henrri
```
</details>

<details>
<summary><b>With cargo install</b></summary>

```bash
cargo install --git https://github.com/edouard-claude/henrri-mcp
```
</details>

<details>
<summary><b>With Docker</b></summary>

```bash
docker run --rm -i \
  -e HENRRI_USERNAME -e HENRRI_PASSWORD \
  ghcr.io/edouard-claude/henrri-mcp:latest
```

The image speaks stdio by default; add `--http 0.0.0.0:8080` (with `HENRRI_HTTP_TOKEN`) to
serve HTTP.
</details>

### Get API access

Henrri gives API access with its developer offer. There are two environments, with separate
accounts:

| Environment | API root | Credits |
| :-- | :-- | :-- |
| Production | `https://api.henrri.io` | charged on issuance, monthly quota |
| Sandbox | `https://api-sandbox.henrri.io` | unlimited, throwaway data |

Three ways to authenticate, in this order of precedence:

1. **A developer client** — `HENRRI_CLIENT_ID` and `HENRRI_CLIENT_SECRET`, exchanged at
   `POST /v1/users/authenticate` and renewed through `POST /v1/users/refresh-token`.
2. **An API user** — `HENRRI_USERNAME` and `HENRRI_PASSWORD`, the OAuth2 `password` grant on
   `POST /v1/users/token` (this is what the sandbox hands out, e.g. `xxxx@sandbox.henrri.net`).
3. **A ready-made token** — `HENRRI_ACCESS_TOKEN`, used as is and never renewed (ten minutes).

Then check everything works:

```bash
HENRRI_ENVIRONMENT=sandbox HENRRI_USERNAME=… HENRRI_PASSWORD=… mcp-henrri --check
```

```console
environment  sandbox
base url     https://api-sandbox.henrri.io
credentials  API user (OAuth2 password grant)
toolsets     account, customers, documents, catalog, payments, revenues, webhooks
tools        89
token        ok
hello world  "Hello World"
user         me@sandbox.henrri.net (16079), role admin
company      My company (947), plan discovery
credits      used 100 of unlimited, unlimited left

ready.
```

## Configure

Everything is read from the environment, once, at startup. A missing or malformed variable
stops the process with a message naming it.

| Variable | Default | |
| :-- | :-- | :-- |
| `HENRRI_USERNAME` / `HENRRI_PASSWORD` | | An API user. `HENRRI_PASSWORD_FILE` for Docker secrets. |
| `HENRRI_CLIENT_ID` / `HENRRI_CLIENT_SECRET` | | Or a developer client. `HENRRI_CLIENT_SECRET_FILE` too. |
| `HENRRI_ACCESS_TOKEN` | | Or a bearer token, never renewed. |
| `HENRRI_ENVIRONMENT` | `production` | `production` or `sandbox` (`staging`). |
| `HENRRI_BASE_URL` | per environment | Override the API root. |
| `HENRRI_TOOLSETS` | all | `account,customers,documents,catalog,payments,revenues,webhooks` |
| `HENRRI_READ_ONLY` | `0` | `1` withholds every tool that is not `readOnlyHint`. |
| `HENRRI_TIMEOUT_SECS` | `30` | Per-request timeout. |
| `HENRRI_PDF_TIMEOUT_SECS` | `120` | Timeout of the PDF routes, which render on demand. |
| `HENRRI_MAX_RETRIES` | `3` | Retries on the rate limiter, `503` and busy finalizations. |
| `HENRRI_DOWNLOAD_DIR` | per-user cache | Where PDFs are saved (`~/Library/Caches/mcp-henrri/downloads`, `~/.cache/…`, `%LOCALAPPDATA%\…`). Created `0700`; files written without following links. |
| `HENRRI_WEBHOOK_SECRET` | | The signing secret from Henrri's webhook settings. |
| `HENRRI_HTTP_BIND` | | Serve HTTP on this address instead of stdio (same as `--http`). |
| `HENRRI_HTTP_TOKEN` | | Bearer token the HTTP transport requires; mandatory off loopback. |
| `HENRRI_LOG` | `info` | Log filter. Logs go to stderr, never to stdout. |

Copy [`.env.example`](.env.example) to `.env` for local work; it is git-ignored.

## Connect a client

<details open>
<summary><b>Claude Desktop</b></summary>

In `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "henrri": {
      "command": "/usr/local/bin/mcp-henrri",
      "env": {
        "HENRRI_USERNAME": "you@example.com",
        "HENRRI_PASSWORD": "…",
        "HENRRI_ENVIRONMENT": "production"
      }
    }
  }
}
```
</details>

<details>
<summary><b>Claude Code</b></summary>

```bash
claude mcp add henrri \
  -e HENRRI_USERNAME=you@example.com -e HENRRI_PASSWORD=… \
  -- /usr/local/bin/mcp-henrri
```
</details>

<details>
<summary><b>Cursor, VS Code, and any stdio client</b></summary>

`.cursor/mcp.json` or `.vscode/mcp.json`:

```json
{
  "servers": {
    "henrri": {
      "type": "stdio",
      "command": "/usr/local/bin/mcp-henrri",
      "env": { "HENRRI_USERNAME": "you@example.com", "HENRRI_PASSWORD": "…" }
    }
  }
}
```
</details>

Tip: a bookkeeper who only reads can run with `HENRRI_READ_ONLY=1`; an assistant that only
chases payments can run with `HENRRI_TOOLSETS=customers,payments`.

## Tools

89 tools in seven toolsets. `mcp-henrri --list-tools` prints the ones your configuration
exposes. **R** reads, **W** writes, **D** is destructive or irreversible and needs
`confirm: true` (so do the two pay tools and `validate_quotation`).

### documents — quotes, invoices, credit notes, lines, PDFs

| Tool | | Route |
| :-- | :-: | :-- |
| `list_documents` | R | `GET /v1/documents` — kinds, state, period, search, `min_id` |
| `list_documents_table` | R | `GET /v1/documents/with-selected-fields` |
| `create_document` | W | `POST /v1/documents` — an empty draft |
| `get_document` | R | `GET /v1/documents/{id}` |
| `get_document_with_all` | R | `GET /v1/documents/{id}/with-all` |
| `get_document_all_included` | R | `GET /v1/documents/{id}/all-included` |
| `update_document` | W | `PUT /v1/documents/{id}` — partial, merged |
| `delete_document` | D | `DELETE /v1/documents/{id}` |
| `finalize_document` | D | `POST /v1/documents/{id}/finalize` — 8 or 2 credits |
| `validate_quotation` | W | `POST /v1/documents/{id}/validate` — 2 credits, needs `confirm` |
| `transform_quote_to_invoice` | W | `POST /v1/documents/{id}/transform-to-invoice` |
| `get_document_tax_details` | R | `GET /v1/documents/{id}/tax-details` |
| `get_document_display` | R | `GET /v1/documents/{id}/display` |
| `get_payment_terms_display` | R | `GET /v1/documents/{id}/paymenttermsdisplay` |
| `set_payment_terms_display` | W | `PUT /v1/documents/{id}/paymenttermsdisplay` — partial |
| `list_document_payment_milestones` | R | `GET /v1/documents/{documentId}/paymentmilestones` |
| `get_document_pdf` | R | `GET /v1/documents/{id}/pdf` — saved to disk, Factur-X option |
| `create_document_pdf_url` | W | `POST /v1/documents/{id}/pdf/url` |
| `download_document_pdf_file` | R | `GET /v1/documents/{id}/pdf/files/{guid}` |
| `next_quote_batch` | W | `GET /v1/documents/next-quote-batch` — moves a server-side cursor |
| `list_document_types` | R | `GET /v1/documenttypes` |
| `list_document_lines` | R | `GET /v1/documents/{documentId}/lines` |
| `add_document_line` | W | `POST /v1/documents/{documentId}/lines` — `kind` resolved to the type id |
| `get_document_line` | R | `GET /v1/documents/{documentId}/lines/{id}` |
| `update_document_line` | W | `PUT /v1/documents/{documentId}/lines/{id}` — partial, merged |
| `delete_document_line` | D | `DELETE /v1/documents/{documentId}/lines/{id}` |
| `move_document_line` | W | `POST /v1/documents/{documentId}/lines/{id}/move` |
| `list_document_line_types` | R | `GET /v1/documentlinetypes` |

### payments — milestones, deposits, payments

| Tool | | Route |
| :-- | :-: | :-- |
| `list_payment_milestones` | R | `GET /v1/paymentmilestones` — paid, overdue, document |
| `list_unpaid_milestones_table` | R | `GET /v1/paymentmilestones/with-selected-fields` |
| `create_payment_milestone` | W | `POST /v1/paymentmilestones` |
| `get_payment_milestone` | R | `GET /v1/paymentmilestones/{id}` |
| `update_payment_milestone` | W | `PUT /v1/paymentmilestones/{id}` — partial |
| `delete_payment_milestone` | D | `DELETE /v1/paymentmilestones/{id}` |
| `pay_payment_milestone` | W | `POST /v1/paymentmilestones/{id}/pay` — needs `confirm` |
| `pay_payment_milestones` | W | `POST /v1/paymentmilestones/pay` — needs `confirm` |
| `get_milestone_payment` | R | `GET /v1/paymentmilestones/{id}/payment` |
| `create_deposit_request` | W | `POST /v1/paymentmilestones/{id}/deposit-request` |
| `create_deposit_settlement` | W | `POST /v1/paymentmilestones/{id}/deposit-settlement` |
| `cancel_deposit` | D | `DELETE /v1/paymentmilestones/{id}/deposit` |
| `list_payment_modes` | R | `GET /v1/paymentmodes` |
| `list_payments` | R | `GET /v1/payments` |
| `get_payment` | R | `GET /v1/payments/{id}` |
| `list_payment_settled_milestones` | R | `GET /v1/payments/{id}/milestones` |

### customers — customers and contacts

| Tool | | Route |
| :-- | :-: | :-- |
| `list_customers` | R | `GET /v1/customers` |
| `list_customers_table` | R | `GET /v1/customers/with-selected-fields` |
| `list_best_customers` | R | `GET /v1/customers/best-sales` |
| `list_last_used_customers` | R | `GET /v1/customers/last-used` |
| `create_customer` | W | `POST /v1/customers` — contact rules and SIRET/BCE checked first |
| `get_customer` | R | `GET /v1/customers/{id}` |
| `update_customer` | W | `PUT /v1/customers/{id}` — partial, merged |
| `delete_customer` | D | `DELETE /v1/customers/{id}` |
| `get_customer_address` | R | `GET /v1/customers/{id}/address` |
| `list_customer_payment_milestones` | R | `GET /v1/customers/{id}/paymentmilestones` |
| `list_customer_contacts` | R | `GET /v1/customers/{id}/contacts` |
| `add_customer_contact` | W | `POST /v1/customers/{id}/contacts` |
| `get_customer_contact` | R | `GET /v1/customers/{id}/contacts/{contactId}` |
| `update_customer_contact` | W | `PUT /v1/customers/{id}/contacts/{contactId}` — partial |
| `delete_customer_contact` | D | `DELETE /v1/customers/{id}/contacts/{contactId}` |

### catalog — items, categories, text items, units

| Tool | | Route |
| :-- | :-: | :-- |
| `list_items` | R | `GET /v1/items` |
| `list_items_table` | R | `GET /v1/items/with-selected-fields` |
| `list_most_used_items` | R | `GET /v1/items/most-used` |
| `list_best_selling_items` | R | `GET /v1/items/best-sales` |
| `create_item` | W | `POST /v1/items` |
| `get_item` | R | `GET /v1/items/{id}` |
| `update_item` | W | `PUT /v1/items/{id}` — partial, merged |
| `delete_item` | D | `DELETE /v1/items/{id}` |
| `list_item_categories` | R | `GET /v1/itemcategories` |
| `list_text_items` | R | `GET /v1/textitems` |
| `create_text_item` | W | `POST /v1/textitems` |
| `get_text_item` | R | `GET /v1/textitems/{id}` |
| `update_text_item` | W | `PUT /v1/textitems/{id}` — partial |
| `delete_text_item` | D | `DELETE /v1/textitems/{id}` |
| `list_units` | R | `GET /v1/units` |
| `create_unit` | W | `POST /v1/units` |
| `get_unit` | R | `GET /v1/units/{id}` |

### account, revenues and webhooks

| Tool | | Route |
| :-- | :-: | :-- |
| `ping` | R | `GET /v1/secures/hello-world` |
| `whoami` | R | `GET /v1/users/me` |
| `get_user` | R | `GET /v1/users/{id}` |
| `get_user_address` | R | `GET /v1/users/{id}/address` |
| `get_my_company` | R | `GET /v1/users/companies` |
| `get_company` | R | `GET /v1/companies/{id}` |
| `get_company_address` | R | `GET /v1/companies/{id}/address` |
| `get_api_usage` | R | the `X-Credits-*` and `X-RateLimit-*` headers |
| `get_yearly_revenue` | R | `GET /v1/revenues/{year}` |
| `get_monthly_revenue` | R | `GET /v1/revenues/{year}/months` |
| `list_webhook_event_types` | R | the specification's 12 webhook events and the test ping |
| `verify_webhook_signature` | R | HMAC-SHA256 of `{timestamp}.{rawBody}`, constant time |
| `list_received_webhook_events` | R | deliveries received at `/webhooks` (HTTP transport) |

`POST /v1/users/authenticate` and `POST /v1/users/refresh-token` are not tools: the client
calls them itself, as does the OAuth2 token endpoint, `POST /v1/users/token`.

## Resources and prompts

| Resource | |
| :-- | :-- |
| `henrri://me` | The user and the company. |
| `henrri://reference` | Document types, line types, payment modes, item categories, units. |
| `henrri://usage` | Credits used and left, rate limit. |
| `henrri://webhooks/events` | The webhook event catalogue. |
| `henrri://webhooks/received` | The deliveries received. |
| `henrri://documents/{document_id}` | A document with all its elements. |
| `henrri://documents/{document_id}/pdf` | A document's PDF, as a blob. |
| `henrri://customers/{customer_id}` | A customer with its payment milestones. |
| `henrri://revenues/{year}` | A year's revenue, total and monthly. |

| Prompt | |
| :-- | :-- |
| `draft_invoice` | From "who" and "what" to a reviewed draft, finalized only on a yes. |
| `chase_unpaid` | What is overdue, per customer, with reminder messages to send yourself. Read-only. |
| `revenue_report` | A year's revenue, month by month, best customers and items. Read-only. |
| `quote_to_invoice` | Check the quote, record the signature, transform, review, finalize on a yes. |

## Henrri, as it actually behaves

Found against the sandbox while building this server, and handled so the model never meets
them. The tests in [`tests/api_contract.rs`](tests/api_contract.rs) pin each one.

- **A `PUT` erases what it is not sent.** Updating a customer's website without its comment
  deletes the comment. Update tools read, merge and write back.
- **Some fields read back as `""` and are refused as `""`.** `vatNumber`, `siret`… An unchanged
  empty string is left out of the write.
- **A customer `PUT` carrying contacts is refused** (`PUT_WITH_CUSTOMER`); a line `PUT` carrying
  its nested item, its type or its computed totals is refused too. They are dropped.
- **A body-less `POST` without `Content-Length` gets an IIS `411`.** `transform-to-invoice`,
  `deposit-request`, `pdf/url`… carry an explicit empty body.
- **A comma-separated list filter is not an error, it is an empty page** (the documentation
  says `400`). Lists are sent as repeated keys.
- **The due date is ignored when a milestone is created**, and honoured when it is updated:
  `create_payment_milestone` sets it in a second call.
- **`confirmFirstOfMonth` is refused in the finalize body** although the
  `FIRST_ACCOUNTING_DOCUMENT_MONTH` error asks for it: it is sent as a query parameter.
- **A new unit is answered with `id: 0`**: `create_unit` finds it by its name.
- **The company-wide milestone lists only show finalized documents**, and a finalized document
  points at a frozen copy of its customer (`parentCustomerId`): a draft's schedule is read
  through the document, a customer's through the copy.
- **The city follows the postal code.** Henrri rewrites it ("Paris 01"); a city that does not
  match the code is blanked. Change both together.
- **`refresh_token` is refused on `/v1/users/token`**; an API user is simply granted again.
- **Two different `429`s.** The per-minute rate limiter is retried; `QUOTA_EXCEEDED` (the
  monthly credits) is not.
- **PDF rendering is slow, and broken on the sandbox** at the time of writing (`500` or no
  answer); the PDF routes get their own, longer timeout.

## Safety

- Irreversible calls are fail-closed: without `confirm: true` nothing leaves the process, and
  the refusal says what would happen. The server instructions and the prompts tell the model to
  show the person and get a yes first.
- `HENRRI_READ_ONLY=1` registers no write tool at all: read-only is decided by each tool's
  `readOnlyHint`, not by a list that could drift.
- Secrets never reach a log or a `Debug` output; logs go to stderr.
- PDFs are written only inside `HENRRI_DOWNLOAD_DIR`, under a sanitized file name.
- Over HTTP, `/mcp` requires `HENRRI_HTTP_TOKEN` (compared in constant time) and the server
  refuses to listen off loopback without one; on loopback, `Host` is validated against DNS
  rebinding.
- Webhook deliveries are accepted only with a valid signature, a timestamp within five minutes,
  and once per delivery id.

## Run it over HTTP

```bash
HENRRI_HTTP_TOKEN=$(openssl rand -hex 32) \
HENRRI_WEBHOOK_SECRET=… \
mcp-henrri --http 127.0.0.1:8080
# MCP at http://127.0.0.1:8080/mcp, webhooks at /webhooks, health at /healthz
```

The Streamable HTTP transport serves many sessions, which share one Henrri token and one
webhook inbox. Behind a reverse proxy, bind `0.0.0.0` (a token is then mandatory) and point
Henrri's webhook callback at `https://<your-host>/webhooks`. Henrri sends each delivery once,
with no retry: the receiver answers `204` immediately, duplicates included.

## Verify

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test                      # unit, contract (fake Henrri), stdio and HTTP protocol tests
```

Against the real sandbox, with your sandbox credentials in `.env` (it builds a draft quote and
deletes it; nothing is finalized):

```bash
set -a; source .env; set +a
HENRRI_SANDBOX_TESTS=1 cargo test --test sandbox
```

Has the API moved? This prints the live operations, in the format of `spec/operations.tsv`
(CI runs it daily against production and sandbox):

```bash
scripts/spec-operations.sh | diff - spec/operations.tsv
```

## Releasing

Bump `version` in `Cargo.toml` and `server.json`, add a `CHANGELOG.md` section, then push a
`vX.Y.Z` tag. The release workflow checks the three agree, builds binaries for five targets,
publishes the multi-arch image to GHCR and the release notes from the changelog.

## Contributing

Issues and pull requests are welcome. Keep the rule the code follows: one route, one tool, and a
test for every behaviour the real API forces on it. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Disclaimer

Henrri is a trademark of its owner. This project is **unofficial**: it is not affiliated with,
endorsed by or supported by Henrri. It is built from Henrri's public v1 API specification and
tested against Henrri's sandbox; behaviour may change when the API does. Finalizing documents
in production has fiscal consequences and costs API credits: you remain responsible for what is
issued in your name. Provided under the MIT license, without warranty.

## License

[MIT](LICENSE) © 2026 Edouard Claude
