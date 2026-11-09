# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-11-09

First release: the whole Henrri v1 API as an unofficial MCP server.

### Added

- 89 tools covering the 87 operations of the Henrri v1 specification: 85 routes,
  one tool each, plus the two authentication routes driven by the client; and
  `get_api_usage` and three webhook tools.
- Seven toolsets (`account`, `customers`, `documents`, `catalog`, `payments`,
  `revenues`, `webhooks`) selectable with `HENRRI_TOOLSETS`, and
  `HENRRI_READ_ONLY`.
- Three ways to authenticate: an API user (OAuth2 password grant), a developer
  client (`/v1/users/authenticate` and `/v1/users/refresh-token`), or a token.
  Ten-minute tokens renewed ahead of expiry, and once more on a 401.
- Partial updates: every `update_*` tool reads, merges and writes back, since
  Henrri's `PUT` erases what it is not sent.
- Finalizing, validating, paying, cancelling deposits and deleting refused
  without `confirm: true`, the refusal stating the document, total and credits.
- API failures returned as `isError` tool results carrying Henrri's RFC 9457
  problem document (`errorCode`, `detail`, field errors, `traceId`) and a hint.
- Retries on the rate limiter, `503` and `FINALIZATION_IN_PROGRESS`; never on
  `QUOTA_EXCEEDED`, never a `POST` that may have run.
- PDFs saved to `HENRRI_DOWNLOAD_DIR`, Factur-X included, with their own timeout.
- Webhooks: the event catalogue, HMAC-SHA256 signature verification, and a
  receiver at `/webhooks` over HTTP with replay protection.
- Five resources, four resource templates and four prompts (`draft_invoice`,
  `chase_unpaid`, `revenue_report`, `quote_to_invoice`).
- stdio and Streamable HTTP transports, the latter behind a bearer token.
- A daily CI check of the tool table against the live production and sandbox
  specifications.
- Prebuilt binaries for macOS, Linux and Windows, and a Docker image on GHCR.
