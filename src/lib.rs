//! A Model Context Protocol server for [Henrri](https://www.henrri.com), the
//! French invoicing software: customers, quotes, invoices, credit notes,
//! payment schedules, deposits, payments and webhooks.
//!
//! **Unofficial.** This project is not made, endorsed or supported by Henrri.
//! It covers the published v1 specification, operation by operation.
//!
//! The crate is split in two halves:
//!
//! * [`client`] is a plain async client for the Henrri v1 HTTP API, with no
//!   MCP in sight: the token lifecycle, retries, credit counters and the API's
//!   problem documents are handled there.
//! * [`server`] and [`tools`] turn that client into an MCP server: tools,
//!   resources and prompts, built on the official [`rmcp`] SDK.
//!
//! Everything is single tenant: one Henrri account, read from the environment,
//! for the whole process.

pub mod client;
pub mod config;
pub mod coverage;
pub mod dates;
pub mod error;
pub mod prompts;
pub mod resources;
pub mod server;
pub mod tools;
pub mod webhooks;

pub use client::Henrri;
pub use config::Config;
pub use error::{Error, Result};
pub use server::HenrriServer;

/// The server name advertised during MCP initialization.
pub const SERVER_NAME: &str = "henrri";

/// The crate version, advertised during MCP initialization.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
