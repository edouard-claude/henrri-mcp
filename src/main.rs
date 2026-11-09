//! The `mcp-henrri` binary: one MCP server over one Henrri account.
//!
//! By default it speaks the stdio transport, which is what desktop MCP clients
//! spawn. `--http` serves the Streamable HTTP transport instead, for a private
//! deployment, and accepts Henrri's webhook deliveries at `/webhooks`. Logs
//! always go to stderr: stdout belongs to the protocol.

#![expect(
    clippy::print_stdout,
    reason = "--help, --version, --check and --list-tools print to stdout and exit before any transport starts; the transports themselves never print"
)]

use std::{net::SocketAddr, sync::Arc, time::SystemTime};

use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode},
    response::IntoResponse as _,
};
use mcp_henrri::{
    Config, Henrri, HenrriServer, SERVER_NAME, SERVER_VERSION,
    config::Secret,
    error::{Error, Result},
    webhooks::{self, Accepted, Inbox, Verdict},
};
use rmcp::{
    ServiceExt,
    transport::{
        io::stdio,
        streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        },
    },
};
use tracing_subscriber::{EnvFilter, fmt};

/// What the command line asked for.
#[derive(Debug, Default)]
struct Args {
    http: Option<String>,
    check: bool,
    list_tools: bool,
    help: bool,
    version: bool,
}

const USAGE: &str = "\
mcp-henrri — an unofficial Model Context Protocol server for Henrri

USAGE:
    mcp-henrri [OPTIONS]

OPTIONS:
    --http <ADDR>     Serve the Streamable HTTP transport on ADDR (e.g. 127.0.0.1:8080)
                      instead of stdio, and receive webhooks at /webhooks.
                      Same as HENRRI_HTTP_BIND.
    --check           Verify the configuration against the API and print a report.
    --list-tools      Print the tools this configuration exposes, then exit.
    -h, --help        Print this help.
    -V, --version     Print the version.

ENVIRONMENT:
    HENRRI_USERNAME          An API user's login (OAuth2 password grant)…
    HENRRI_PASSWORD          …and its password. HENRRI_PASSWORD_FILE for Docker secrets.
    HENRRI_CLIENT_ID         Or a developer client id (POST /v1/users/authenticate)…
    HENRRI_CLIENT_SECRET     …and its secret. HENRRI_CLIENT_SECRET_FILE for Docker secrets.
    HENRRI_ACCESS_TOKEN      Or a ready-made bearer token, never renewed.
    HENRRI_ENVIRONMENT       production (default) or sandbox.
    HENRRI_BASE_URL          Override the API root (default https://api.henrri.io).
    HENRRI_TOOLSETS          account,customers,documents,catalog,payments,revenues,webhooks or all
    HENRRI_READ_ONLY         1 to withhold every write tool.
    HENRRI_TIMEOUT_SECS      Per-request timeout. Default 30.
    HENRRI_PDF_TIMEOUT_SECS  Timeout of the PDF routes. Default 120.
    HENRRI_MAX_RETRIES       Retries on the rate limiter, 503 and busy finalization. Default 3.
    HENRRI_DOWNLOAD_DIR      Where PDFs are saved. Default: a per-user cache directory.
    HENRRI_WEBHOOK_SECRET    The webhook signing secret, to verify deliveries.
    HENRRI_HTTP_BIND         Listen address for the HTTP transport.
    HENRRI_HTTP_TOKEN        Bearer token the HTTP transport requires.
    HENRRI_LOG               Log filter, e.g. info, debug, mcp_henrri=debug.

This project is not affiliated with Henrri.
Documentation: https://github.com/edouard-claude/henrri-mcp";

fn main() -> std::process::ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("mcp-henrri: {err}");
            return std::process::ExitCode::from(2);
        }
    };

    if args.help {
        println!("{USAGE}");
        return std::process::ExitCode::SUCCESS;
    }
    if args.version {
        println!("{SERVER_NAME} {SERVER_VERSION}");
        return std::process::ExitCode::SUCCESS;
    }

    init_tracing();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("mcp-henrri: could not start the async runtime: {err}");
            return std::process::ExitCode::FAILURE;
        }
    };

    match runtime.block_on(run(args)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(%err, "fatal");
            eprintln!("mcp-henrri: {err}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<()> {
    let mut config = Config::from_env()?;
    if let Some(addr) = &args.http {
        let bind = addr.parse::<SocketAddr>().map_err(|e| {
            Error::config(format!(
                "--http expects an address like 127.0.0.1:8080: {e}"
            ))
        })?;
        if !bind.ip().is_loopback() && config.http_token.is_none() {
            return Err(Error::config(format!(
                "--http {bind} is reachable from outside this machine: set HENRRI_HTTP_TOKEN, or bind to 127.0.0.1"
            )));
        }
        config.http_bind = Some(bind);
    }

    if args.list_tools {
        let server = HenrriServer::new(config)?;
        print_tools(&server);
        return Ok(());
    }

    if args.check {
        return check(config).await;
    }

    match config.http_bind {
        Some(bind) => serve_http(config, bind).await,
        None => serve_stdio(config).await,
    }
}

/// Serve the stdio transport, which is what a desktop client spawns.
async fn serve_stdio(config: Config) -> Result<()> {
    let toolsets = toolset_list(&config);
    let server = HenrriServer::new(config)?;
    tracing::info!(
        version = SERVER_VERSION,
        environment = server.config().environment.as_str(),
        tools = server.tools().len(),
        toolsets,
        read_only = server.config().read_only(),
        "serving Henrri over stdio"
    );
    let running = server
        .serve(stdio())
        .await
        .map_err(|e| Error::Transport(format!("stdio transport: {e}")))?;
    running
        .waiting()
        .await
        .map_err(|e| Error::Transport(format!("stdio session: {e}")))?;
    Ok(())
}

/// What the HTTP routes share.
#[derive(Clone)]
struct HttpState {
    inbox: Arc<Inbox>,
    webhook_secret: Option<Secret>,
}

/// Serve the Streamable HTTP transport on `bind`.
async fn serve_http(config: Config, bind: SocketAddr) -> Result<()> {
    let token = config.http_token.clone();
    let state = HttpState {
        inbox: Arc::new(Inbox::default()),
        webhook_secret: config.webhook_secret.clone(),
    };
    // One client for every session, so they share one token instead of each
    // asking Henrri for its own; one inbox, so every session sees the webhooks.
    let client = Arc::new(Henrri::new(config)?);
    let inbox = state.inbox.clone();
    let service = StreamableHttpService::new(
        move || Ok(HenrriServer::with_client(client.clone(), inbox.clone())),
        Arc::new(LocalSessionManager::default()),
        // Host validation guards a local server against DNS rebinding. Behind a
        // reverse proxy the Host header is the public domain, which this process
        // cannot know, so off loopback the bearer token is the guard instead,
        // and the configuration refuses that bind without one.
        if bind.ip().is_loopback() {
            StreamableHttpServerConfig::default().with_allowed_hosts([
                "localhost",
                "127.0.0.1",
                "::1",
                &bind.to_string(),
            ])
        } else {
            StreamableHttpServerConfig::default().disable_allowed_hosts()
        },
    );

    let app = axum::Router::new()
        .route(
            "/healthz",
            axum::routing::get(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }),
        )
        .route(
            "/webhooks",
            axum::routing::post(receive_webhook).with_state(state),
        )
        .route_service("/mcp", service)
        .layer(axum::extract::DefaultBodyLimit::max(
            webhooks::MAX_BODY_BYTES * 16,
        ))
        .layer(axum::middleware::from_fn(move |request, next| {
            let token = token.clone();
            async move { authorize(token, request, next).await }
        }));

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|e| Error::config(format!("cannot listen on {bind}: {e}")))?;
    tracing::info!(%bind, "serving Henrri over Streamable HTTP at /mcp, webhooks at /webhooks");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutting down");
        })
        .await
        .map_err(|e| Error::Transport(format!("http server: {e}")))?;
    Ok(())
}

/// Accept one webhook delivery: verify, deduplicate, store, answer 2xx fast.
///
/// Henrri never retries, so a 2xx is answered for every authentic delivery,
/// duplicates included; anything unsigned or stale is refused.
async fn receive_webhook(
    axum::extract::State(state): axum::extract::State<HttpState>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let refuse = |status: StatusCode, reason: &str| {
        (status, axum::Json(serde_json::json!({ "error": reason }))).into_response()
    };
    let Some(secret) = state.webhook_secret.as_ref() else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "HENRRI_WEBHOOK_SECRET is not set on this server",
        );
    };
    if body.len() > webhooks::MAX_BODY_BYTES {
        return refuse(StatusCode::PAYLOAD_TOO_LARGE, "delivery too large");
    }
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    };
    let verdict = webhooks::verify(
        secret.expose(),
        header(webhooks::TIMESTAMP_HEADER),
        &body,
        header(webhooks::SIGNATURE_HEADER),
        webhooks::DEFAULT_TOLERANCE,
        SystemTime::now(),
    );
    if verdict != Verdict::Valid {
        tracing::warn!(?verdict, "webhook delivery refused");
        return refuse(StatusCode::UNAUTHORIZED, verdict.explain());
    }
    let Some(mut envelope) = webhooks::parse_envelope(&body) else {
        return refuse(
            StatusCode::BAD_REQUEST,
            "the body is not a Henrri webhook envelope",
        );
    };
    if envelope.delivery.is_none() {
        let delivery = header(webhooks::DELIVERY_HEADER);
        envelope.delivery = (!delivery.is_empty()).then(|| delivery.to_owned());
    }
    let event = envelope.event.clone().unwrap_or_default();
    let accepted = state.inbox.accept(envelope);
    tracing::info!(
        event,
        duplicate = (accepted == Accepted::Duplicate),
        "webhook received"
    );
    StatusCode::NO_CONTENT.into_response()
}

/// The HTTP transport's guard: a bearer token, when one is configured.
///
/// One Henrri account means one identity, so this is not authentication of a
/// person: it keeps a private deployment from being an open proxy to a
/// company's invoices. Webhooks carry their own signature instead.
async fn authorize(
    expected: Option<Secret>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let Some(expected) = expected else {
        return next.run(request).await;
    };
    if matches!(request.uri().path(), "/healthz" | "/webhooks") {
        return next.run(request).await;
    }
    let presented = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim);
    match presented {
        Some(token) if constant_time_eq(token.as_bytes(), expected.expose().as_bytes()) => {
            next.run(request).await
        }
        _ => {
            let mut response = (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({
                    "error": "unauthorized",
                    "error_description": "this endpoint requires the bearer token set in HENRRI_HTTP_TOKEN",
                })),
            )
                .into_response();
            response.headers_mut().insert(
                axum::http::header::WWW_AUTHENTICATE,
                axum::http::HeaderValue::from_static("Bearer realm=\"mcp-henrri\""),
            );
            response
        }
    }
}

/// Compare two secrets without leaking their common prefix through timing.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

fn toolset_list(config: &Config) -> String {
    config
        .toolsets
        .iter()
        .map(|t| t.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

/// Verify the configuration against the live API and report what was found.
async fn check(config: Config) -> Result<()> {
    println!("environment  {}", config.environment.as_str());
    println!("base url     {}", config.base_url);
    println!("credentials  {}", config.credentials.describe());
    println!("toolsets     {}", toolset_list(&config));
    println!("read only    {}", config.read_only());
    println!(
        "webhooks     {}",
        if config.webhook_secret.is_some() {
            "secret set"
        } else {
            "no HENRRI_WEBHOOK_SECRET"
        }
    );
    println!(
        "downloads    {}",
        config.download_dir.as_ref().map_or_else(
            || "none: set HENRRI_DOWNLOAD_DIR to save PDFs".to_owned(),
            |d| d.display().to_string()
        )
    );

    let server = HenrriServer::new(config)?;
    println!("tools        {}", server.tools().len());

    server.client().access_token().await?;
    println!("token        ok");

    let hello: serde_json::Value = server
        .client()
        .call(mcp_henrri::client::Request::get("/v1/secures/hello-world"))
        .await?;
    println!("hello world  {hello}");

    let me: serde_json::Value = server
        .client()
        .call(mcp_henrri::client::Request::get("/v1/users/me"))
        .await?;
    let field = |pointer: &str| {
        me.pointer(pointer).map_or_else(
            || "?".to_owned(),
            |v| v.as_str().map_or_else(|| v.to_string(), str::to_owned),
        )
    };
    println!(
        "user         {} ({}), role {}",
        field("/user/email"),
        field("/user/id"),
        field("/user/role")
    );
    println!(
        "company      {} ({}), plan {}",
        field("/company/name"),
        field("/company/id"),
        field("/company/subscriptionPlan")
    );
    let usage = server.client().usage();
    println!(
        "credits      used {} of {}, {} left",
        usage.credits_used.as_deref().unwrap_or("?"),
        usage.credits_limit.as_deref().unwrap_or("?"),
        usage.credits_remaining.as_deref().unwrap_or("?")
    );

    println!("\nready.");
    Ok(())
}

/// Print the tool table, which is also how the README's table is kept honest.
fn print_tools(server: &HenrriServer) {
    let tools = server.tools();
    println!("{} tools", tools.len());
    for tool in tools {
        let hint = |f: fn(&rmcp::model::ToolAnnotations) -> Option<bool>| {
            tool.annotations.as_ref().and_then(f).unwrap_or(false)
        };
        let kind = match (hint(|a| a.read_only_hint), hint(|a| a.destructive_hint)) {
            (true, _) => "read ",
            (false, true) => "DEL  ",
            (false, false) => "write",
        };
        let description = tool
            .description
            .as_deref()
            .unwrap_or("")
            .split(". ")
            .next()
            .unwrap_or("")
            .replace('\n', " ");
        println!("  {kind} {:<34} {}", tool.name, description.trim());
    }
}

/// Logs go to stderr, never to stdout: stdout is the protocol's channel.
fn init_tracing() {
    let filter = EnvFilter::try_from_env("HENRRI_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .with_ansi(false)
        .try_init();
}

fn parse_args(args: impl Iterator<Item = String>) -> std::result::Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--http" => {
                parsed.http = Some(
                    args.next()
                        .ok_or_else(|| "--http needs an address like 127.0.0.1:8080".to_owned())?,
                );
            }
            other if other.starts_with("--http=") => {
                parsed.http = Some(other.trim_start_matches("--http=").to_owned());
            }
            "--check" => parsed.check = true,
            "--list-tools" => parsed.list_tools = true,
            "-h" | "--help" => parsed.help = true,
            "-V" | "--version" => parsed.version = true,
            other => {
                return Err(format!(
                    "unknown argument {other:?}. Run --help for the options."
                ));
            }
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot fail loudly is not a test"
    )]
    use mcp_henrri::config::Toolset;

    use super::*;

    #[test]
    fn arguments_parse_in_both_spellings() {
        let args = parse_args(["--http".to_owned(), "127.0.0.1:9000".to_owned()].into_iter())
            .expect("parse");
        assert_eq!(args.http.as_deref(), Some("127.0.0.1:9000"));
        let args = parse_args(["--http=0.0.0.0:80".to_owned()].into_iter()).expect("parse");
        assert_eq!(args.http.as_deref(), Some("0.0.0.0:80"));
    }

    #[test]
    fn an_unknown_argument_points_at_the_help() {
        let err = parse_args(["--nope".to_owned()].into_iter()).unwrap_err();
        assert!(err.contains("--help"), "{err}");
    }

    #[test]
    fn the_help_text_documents_every_toolset() {
        for toolset in Toolset::ALL {
            assert!(
                USAGE.contains(toolset.as_str()),
                "the help text omits {toolset}"
            );
        }
    }

    #[test]
    fn secrets_are_compared_without_a_timing_shortcut() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
