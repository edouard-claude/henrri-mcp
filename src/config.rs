//! Configuration, read once from the environment at startup.
//!
//! Nothing here reads the environment lazily: a missing or malformed variable
//! stops the process with a message that names the variable, rather than
//! surfacing as a confusing tool failure three calls later.

use std::{
    collections::BTreeSet, fmt, net::SocketAddr, path::PathBuf, str::FromStr, time::Duration,
};

use crate::error::{Error, Result};

/// The production API root. Paths carry the `/v1` prefix themselves.
pub const PRODUCTION_BASE_URL: &str = "https://api.henrri.io";
/// The sandbox ("staging") API root, for testing with throwaway data.
pub const SANDBOX_BASE_URL: &str = "https://api-sandbox.henrri.io";

/// A string that never shows up in a log or a panic message.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wrap a value so it is redacted when formatted.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value itself. Only the HTTP and signature layers should need this.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// Which Henrri the process talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    /// Real companies, real invoices, credits charged on issuance.
    Production,
    /// Henrri's sandbox, with its own accounts and no credit limit.
    Sandbox,
}

impl Environment {
    /// The name used in `HENRRI_ENVIRONMENT`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Sandbox => "sandbox",
        }
    }
}

/// How the process authenticates against Henrri.
#[derive(Debug, Clone)]
pub enum Credentials {
    /// A developer API client: `POST /v1/users/authenticate` with a JSON body,
    /// renewed through `POST /v1/users/refresh-token`.
    Client {
        /// The client id.
        client_id: String,
        /// The client secret.
        client_secret: Secret,
    },
    /// A Henrri API user (login and password): OAuth2 `password` grant on
    /// `POST /v1/users/token`, granted again when the token expires.
    Password {
        /// The login, an email address.
        username: String,
        /// The password.
        password: Secret,
    },
    /// A ready-made access token, used as is and never renewed.
    Token(Secret),
}

impl Credentials {
    /// A word for `--check` and the logs.
    pub fn describe(&self) -> &'static str {
        match self {
            Self::Client { .. } => "API client (client id and secret)",
            Self::Password { .. } => "API user (OAuth2 password grant)",
            Self::Token(_) => "static access token",
        }
    }
}

/// A group of tools, so a deployment can expose only what it needs.
///
/// Tool count is context: an assistant that only chases unpaid invoices has no
/// use for the catalogue administration tools, and every unused schema is
/// tokens spent on every request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Toolset {
    /// The user, the company, connectivity and credit usage.
    Account,
    /// Customers and their contacts.
    Customers,
    /// Documents (quotes, invoices, credit notes…), their lines and PDFs.
    Documents,
    /// The catalogue: items, item categories, text items and units.
    Catalog,
    /// Payment milestones, deposits, payments and payment modes.
    Payments,
    /// Revenue statistics.
    Revenues,
    /// Webhook events: catalogue, signature verification, received events.
    Webhooks,
}

impl Toolset {
    /// Every toolset, in the order they are documented.
    pub const ALL: [Self; 7] = [
        Self::Account,
        Self::Customers,
        Self::Documents,
        Self::Catalog,
        Self::Payments,
        Self::Revenues,
        Self::Webhooks,
    ];

    /// The name used in `HENRRI_TOOLSETS`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Customers => "customers",
            Self::Documents => "documents",
            Self::Catalog => "catalog",
            Self::Payments => "payments",
            Self::Revenues => "revenues",
            Self::Webhooks => "webhooks",
        }
    }
}

impl FromStr for Toolset {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "account" | "user" | "users" | "company" | "companies" => Ok(Self::Account),
            "customers" | "customer" | "clients" | "contacts" => Ok(Self::Customers),
            "documents" | "document" | "invoices" | "quotes" | "lines" => Ok(Self::Documents),
            "catalog" | "catalogue" | "items" | "units" | "textitems" => Ok(Self::Catalog),
            "payments" | "payment" | "milestones" | "deposits" => Ok(Self::Payments),
            "revenues" | "revenue" | "statistics" => Ok(Self::Revenues),
            "webhooks" | "webhook" | "events" => Ok(Self::Webhooks),
            other => Err(Error::config(format!(
                "HENRRI_TOOLSETS: unknown toolset {other:?}, expected one of account, customers, documents, catalog, payments, revenues, webhooks, or all"
            ))),
        }
    }
}

impl fmt::Display for Toolset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether write tools are registered at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Every tool, reads and writes.
    ReadWrite,
    /// Only the tools that annotate themselves `readOnlyHint`.
    ReadOnly,
}

/// Everything the process needs to know, resolved from the environment.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Config {
    /// How requests are authenticated.
    pub credentials: Credentials,
    /// Production or sandbox, for messages; `base_url` is what is used.
    pub environment: Environment,
    /// The API root, without the `/v1` prefix. No trailing slash.
    pub base_url: String,
    /// The toolsets to expose.
    pub toolsets: BTreeSet<Toolset>,
    /// Whether write tools are exposed.
    pub access: Access,
    /// Per-request timeout.
    pub timeout: Duration,
    /// Timeout of the PDF endpoints, which render on demand and are slow.
    pub pdf_timeout: Duration,
    /// How many times a transient failure is retried.
    pub max_retries: u32,
    /// Where PDF files are written.
    pub download_dir: PathBuf,
    /// The secret Henrri signs webhook deliveries with.
    pub webhook_secret: Option<Secret>,
    /// Listen address for the Streamable HTTP transport, when not stdio.
    pub http_bind: Option<SocketAddr>,
    /// Bearer token required by the HTTP transport.
    pub http_token: Option<Secret>,
}

impl Config {
    /// Read the configuration from the process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_getter(|key| std::env::var(key).ok())
    }

    /// Read the configuration from an arbitrary source, which is what the tests
    /// use.
    pub fn from_getter(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let get_trimmed = |key: &str| -> Option<String> {
            get(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let from_file = |key: &str| -> Result<Option<String>> {
            if let Some(value) = get_trimmed(key) {
                return Ok(Some(value));
            }
            let Some(path) = get_trimmed(&format!("{key}_FILE")) else {
                return Ok(None);
            };
            let value = std::fs::read_to_string(&path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            Ok(Some(value.trim().to_owned()).filter(|v| !v.is_empty()))
        };

        let credentials = credentials(
            get_trimmed("HENRRI_CLIENT_ID"),
            from_file("HENRRI_CLIENT_SECRET")?,
            get_trimmed("HENRRI_USERNAME"),
            from_file("HENRRI_PASSWORD")?,
            from_file("HENRRI_ACCESS_TOKEN")?,
        )?;

        let environment = match get_trimmed("HENRRI_ENVIRONMENT")
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            None | Some("production" | "prod" | "live") => Environment::Production,
            Some("sandbox" | "staging" | "test") => Environment::Sandbox,
            Some(other) => {
                return Err(Error::config(format!(
                    "HENRRI_ENVIRONMENT must be production or sandbox, got {other:?}"
                )));
            }
        };
        let default_base = match environment {
            Environment::Production => PRODUCTION_BASE_URL,
            Environment::Sandbox => SANDBOX_BASE_URL,
        };
        let base_url = url_var(
            "HENRRI_BASE_URL",
            get_trimmed("HENRRI_BASE_URL"),
            default_base,
        )?;

        let toolsets = parse_toolsets(get_trimmed("HENRRI_TOOLSETS").as_deref())?;
        let access = if parse_bool(
            "HENRRI_READ_ONLY",
            get_trimmed("HENRRI_READ_ONLY").as_deref(),
            false,
        )? {
            Access::ReadOnly
        } else {
            Access::ReadWrite
        };

        let http_bind = match get_trimmed("HENRRI_HTTP_BIND") {
            Some(addr) => Some(addr.parse::<SocketAddr>().map_err(|e| {
                Error::config(format!("HENRRI_HTTP_BIND is not a socket address: {e}"))
            })?),
            None => None,
        };
        let http_token = from_file("HENRRI_HTTP_TOKEN")?.map(Secret::new);
        if let Some(addr) = http_bind.filter(|a| !a.ip().is_loopback() && http_token.is_none()) {
            return Err(Error::config(format!(
                "HENRRI_HTTP_BIND is {addr}, which is reachable from outside this machine: set HENRRI_HTTP_TOKEN so the endpoint requires a bearer token, or bind to 127.0.0.1"
            )));
        }

        let download_dir = get_trimmed("HENRRI_DOWNLOAD_DIR")
            .map_or_else(|| default_download_dir(&get), PathBuf::from);

        Ok(Self {
            credentials,
            environment,
            base_url,
            toolsets,
            access,
            timeout: Duration::from_secs(parse_u64(
                "HENRRI_TIMEOUT_SECS",
                get_trimmed("HENRRI_TIMEOUT_SECS").as_deref(),
                30,
                1,
            )?),
            pdf_timeout: Duration::from_secs(parse_u64(
                "HENRRI_PDF_TIMEOUT_SECS",
                get_trimmed("HENRRI_PDF_TIMEOUT_SECS").as_deref(),
                120,
                1,
            )?),
            max_retries: u32::try_from(parse_u64(
                "HENRRI_MAX_RETRIES",
                get_trimmed("HENRRI_MAX_RETRIES").as_deref(),
                3,
                0,
            )?)
            .map_err(|_| Error::config("HENRRI_MAX_RETRIES is too large"))?,
            download_dir,
            webhook_secret: from_file("HENRRI_WEBHOOK_SECRET")?.map(Secret::new),
            http_bind,
            http_token,
        })
    }

    /// True when the given toolset is exposed.
    pub fn has(&self, toolset: Toolset) -> bool {
        self.toolsets.contains(&toolset)
    }

    /// True when write tools are withheld.
    pub fn read_only(&self) -> bool {
        self.access == Access::ReadOnly
    }
}

/// A per-user directory for PDFs, never the shared temp directory: the
/// platform's cache directory, then the home directory.
fn default_download_dir(get: &impl Fn(&str) -> Option<String>) -> PathBuf {
    let var = |key: &str| get(key).filter(|v| !v.trim().is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        if let Some(local) = var("LOCALAPPDATA") {
            return local.join("mcp-henrri").join("downloads");
        }
    } else if cfg!(target_os = "macos") {
        if let Some(home) = var("HOME") {
            return home.join("Library/Caches/mcp-henrri/downloads");
        }
    } else if let Some(cache) = var("XDG_CACHE_HOME") {
        return cache.join("mcp-henrri/downloads");
    } else if let Some(home) = var("HOME") {
        return home.join(".cache/mcp-henrri/downloads");
    }
    std::env::temp_dir().join(format!("mcp-henrri-{}", std::process::id()))
}

/// Pick the authentication method from what is set, and refuse half of one.
fn credentials(
    client_id: Option<String>,
    client_secret: Option<String>,
    username: Option<String>,
    password: Option<String>,
    token: Option<String>,
) -> Result<Credentials> {
    match (client_id, client_secret) {
        (Some(client_id), Some(secret)) => {
            return Ok(Credentials::Client {
                client_id,
                client_secret: Secret::new(secret),
            });
        }
        (Some(_), None) => {
            return Err(Error::config(
                "HENRRI_CLIENT_ID is set but HENRRI_CLIENT_SECRET is not (or HENRRI_CLIENT_SECRET_FILE pointing at a file holding it).",
            ));
        }
        (None, Some(_)) => {
            return Err(Error::config(
                "HENRRI_CLIENT_SECRET is set but HENRRI_CLIENT_ID is not.",
            ));
        }
        (None, None) => {}
    }
    match (username, password) {
        (Some(username), Some(password)) => {
            return Ok(Credentials::Password {
                username,
                password: Secret::new(password),
            });
        }
        (Some(_), None) => {
            return Err(Error::config(
                "HENRRI_USERNAME is set but HENRRI_PASSWORD is not (or HENRRI_PASSWORD_FILE pointing at a file holding it).",
            ));
        }
        (None, Some(_)) => {
            return Err(Error::config(
                "HENRRI_PASSWORD is set but HENRRI_USERNAME is not.",
            ));
        }
        (None, None) => {}
    }
    token.map(|t| Credentials::Token(Secret::new(t))).ok_or_else(|| {
        Error::config(
            "no Henrri credentials: set HENRRI_USERNAME and HENRRI_PASSWORD (an API user), or HENRRI_CLIENT_ID and HENRRI_CLIENT_SECRET (a developer client), or HENRRI_ACCESS_TOKEN. Henrri provides API access from its developer offer; the sandbox at api-sandbox.henrri.io has its own accounts.",
        )
    })
}

fn url_var(name: &str, raw: Option<String>, default: &str) -> Result<String> {
    let url = raw
        .unwrap_or_else(|| default.to_owned())
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_owned();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(Error::config(format!(
            "{name} must start with http:// or https://, got {url:?}"
        )));
    }
    Ok(url)
}

fn parse_toolsets(raw: Option<&str>) -> Result<BTreeSet<Toolset>> {
    let Some(raw) = raw else {
        return Ok(Toolset::ALL.into_iter().collect());
    };
    let mut set = BTreeSet::new();
    for part in raw.split(',').filter(|p| !p.trim().is_empty()) {
        if part.trim().eq_ignore_ascii_case("all") {
            set.extend(Toolset::ALL);
            continue;
        }
        set.insert(part.parse::<Toolset>()?);
    }
    if set.is_empty() {
        return Err(Error::config(
            "HENRRI_TOOLSETS is set but selects no toolset",
        ));
    }
    Ok(set)
}

fn parse_bool(name: &str, raw: Option<&str>, default: bool) -> Result<bool> {
    match raw {
        None => Ok(default),
        Some(v) => match v.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            other => Err(Error::config(format!(
                "{name} expects a boolean (1/0, true/false), got {other:?}"
            ))),
        },
    }
}

fn parse_u64(name: &str, raw: Option<&str>, default: u64, min: u64) -> Result<u64> {
    match raw {
        None => Ok(default),
        Some(v) => v.parse::<u64>().ok().filter(|n| *n >= min).ok_or_else(|| {
            Error::config(format!(
                "{name} must be an integer of at least {min}, got {v:?}"
            ))
        }),
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

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| owned.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    const USER: [(&str, &str); 2] = [
        ("HENRRI_USERNAME", "me@example.com"),
        ("HENRRI_PASSWORD", "pw"),
    ];

    fn with(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
        let mut pairs = USER.to_vec();
        pairs.extend_from_slice(extra);
        pairs
    }

    #[test]
    fn missing_credentials_name_every_way_to_authenticate() {
        let message = Config::from_getter(env(&[])).unwrap_err().to_string();
        for var in ["HENRRI_USERNAME", "HENRRI_CLIENT_ID", "HENRRI_ACCESS_TOKEN"] {
            assert!(message.contains(var), "{message}");
        }
    }

    #[test]
    fn half_a_pair_is_refused() {
        let err = Config::from_getter(env(&[("HENRRI_CLIENT_ID", "id")])).unwrap_err();
        assert!(err.to_string().contains("HENRRI_CLIENT_SECRET"), "{err}");
        let err = Config::from_getter(env(&[("HENRRI_USERNAME", "u")])).unwrap_err();
        assert!(err.to_string().contains("HENRRI_PASSWORD"), "{err}");
    }

    #[test]
    fn a_client_wins_over_a_user_and_a_user_over_a_token() {
        let cfg = Config::from_getter(env(&with(&[
            ("HENRRI_CLIENT_ID", "id"),
            ("HENRRI_CLIENT_SECRET", "s"),
            ("HENRRI_ACCESS_TOKEN", "t"),
        ])))
        .unwrap();
        assert!(matches!(cfg.credentials, Credentials::Client { .. }));
        let cfg = Config::from_getter(env(&with(&[("HENRRI_ACCESS_TOKEN", "t")]))).unwrap();
        assert!(matches!(cfg.credentials, Credentials::Password { .. }));
        let cfg = Config::from_getter(env(&[("HENRRI_ACCESS_TOKEN", "t")])).unwrap();
        assert!(matches!(cfg.credentials, Credentials::Token(_)));
    }

    #[test]
    fn defaults_are_the_documented_ones() {
        let cfg = Config::from_getter(env(&USER)).unwrap();
        assert_eq!(cfg.base_url, PRODUCTION_BASE_URL);
        assert_eq!(cfg.toolsets.len(), Toolset::ALL.len());
        assert!(!cfg.read_only());
        assert_eq!(cfg.timeout, Duration::from_secs(30));
        assert_eq!(cfg.pdf_timeout, Duration::from_secs(120));
        assert_eq!(cfg.max_retries, 3);
        assert!(cfg.webhook_secret.is_none());
    }

    #[test]
    fn pdfs_default_to_a_per_user_directory() {
        let cfg = Config::from_getter(env(&with(&[
            ("HOME", "/home/ada"),
            ("XDG_CACHE_HOME", "/home/ada/.cache"),
            ("LOCALAPPDATA", "C:/Users/ada/AppData/Local"),
        ])))
        .unwrap();
        let dir = cfg.download_dir.display().to_string();
        assert!(dir.contains("ada") && dir.contains("mcp-henrri"), "{dir}");
    }

    #[test]
    fn the_sandbox_switches_the_url() {
        let cfg = Config::from_getter(env(&with(&[("HENRRI_ENVIRONMENT", "sandbox")]))).unwrap();
        assert_eq!(cfg.base_url, SANDBOX_BASE_URL);
        let cfg = Config::from_getter(env(&with(&[("HENRRI_ENVIRONMENT", "staging")]))).unwrap();
        assert_eq!(cfg.environment, Environment::Sandbox);
    }

    #[test]
    fn secrets_are_never_printed() {
        let cfg = Config::from_getter(env(&[
            ("HENRRI_USERNAME", "me@example.com"),
            ("HENRRI_PASSWORD", "super-secret"),
            ("HENRRI_WEBHOOK_SECRET", "hook-secret"),
        ]))
        .unwrap();
        let printed = format!("{cfg:?}");
        assert!(!printed.contains("super-secret"), "{printed}");
        assert!(!printed.contains("hook-secret"), "{printed}");
    }

    #[test]
    fn a_base_url_loses_its_trailing_slash_and_version() {
        let cfg = Config::from_getter(env(&with(&[(
            "HENRRI_BASE_URL",
            "https://example.test/v1/",
        )])))
        .unwrap();
        assert_eq!(cfg.base_url, "https://example.test");
        let err =
            Config::from_getter(env(&with(&[("HENRRI_BASE_URL", "api.henrri.io")]))).unwrap_err();
        assert!(err.to_string().contains("http://"), "{err}");
    }

    #[test]
    fn toolsets_accept_aliases_and_all() {
        let cfg =
            Config::from_getter(env(&with(&[("HENRRI_TOOLSETS", "invoices, clients")]))).unwrap();
        assert!(cfg.has(Toolset::Documents));
        assert!(cfg.has(Toolset::Customers));
        assert!(!cfg.has(Toolset::Payments));
        let err = Config::from_getter(env(&with(&[("HENRRI_TOOLSETS", "crm")]))).unwrap_err();
        assert!(err.to_string().contains("catalog"), "{err}");
    }

    #[test]
    fn binding_http_off_loopback_without_a_token_is_refused() {
        let err =
            Config::from_getter(env(&with(&[("HENRRI_HTTP_BIND", "0.0.0.0:8080")]))).unwrap_err();
        assert!(err.to_string().contains("HENRRI_HTTP_TOKEN"), "{err}");
        assert!(Config::from_getter(env(&with(&[("HENRRI_HTTP_BIND", "127.0.0.1:8080")]))).is_ok());
    }

    #[test]
    fn a_zero_timeout_is_refused_but_zero_retries_is_fine() {
        let err = Config::from_getter(env(&with(&[("HENRRI_TIMEOUT_SECS", "0")]))).unwrap_err();
        assert!(err.to_string().contains("HENRRI_TIMEOUT_SECS"), "{err}");
        let cfg = Config::from_getter(env(&with(&[("HENRRI_MAX_RETRIES", "0")]))).unwrap();
        assert_eq!(cfg.max_retries, 0);
    }
}
