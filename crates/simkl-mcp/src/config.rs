//! The server's configuration: defaults, then an optional TOML file, then
//! `SIMKL_MCP_*` environment variables, with `__` between nested keys
//! (`SIMKL_MCP_CLIENT__CLIENT_ID`, `SIMKL_MCP_AUTH__REFRESH_MARGIN`).
//!
//! Secrets never come from the file: [`Secrets`] reads them from the
//! environment only.

use std::env;
use std::net::SocketAddr;
use std::path::Path;

use anyhow::Context;
use figment::Figment;
use figment::providers::{Env, Format, Serialized, Toml};
use secrecy::SecretString;
use serde::Deserialize;
use simkl_api::ClientConfig;
use url::Url;

use crate::auth::{AuthConfig, VaultKey};

const ENV_PREFIX: &str = "SIMKL_MCP_";
/// Environment variables under [`ENV_PREFIX`] that are not configuration:
/// the secrets, and where the configuration file is.
const NOT_CONFIGURATION: [&str; 3] = ["client_secret", "vault_key", "config"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Where MCP clients reach this server, such as `https://simkl.xwx.cat`.
    /// The issuer, the `/mcp` resource and the Simkl callback derive from it.
    pub public_url: Url,
    #[serde(default = "default_bind")]
    pub bind: SocketAddr,
    /// The SQLite database, such as `sqlite:///data/simkl-mcp.db`.
    #[serde(default = "default_database")]
    pub database: String,
    /// The Simkl API client. `client_id` is our Simkl app's; `app_name` and
    /// `app_version` default to this crate's.
    pub client: ClientConfig,
    #[serde(default)]
    pub auth: AuthConfig,
}

fn default_bind() -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], 8080))
}

fn default_database() -> String {
    "sqlite://simkl-mcp.db".to_owned()
}

impl Config {
    /// Reads the configuration, from `file` too when given.
    pub fn load(file: Option<&Path>) -> anyhow::Result<Config> {
        let mut figment = Figment::new()
            .merge(Serialized::default(
                "client.app_name",
                env!("CARGO_PKG_NAME"),
            ))
            .merge(Serialized::default(
                "client.app_version",
                env!("CARGO_PKG_VERSION"),
            ));
        if let Some(file) = file {
            figment = figment.merge(Toml::file_exact(file));
        }
        figment
            .merge(
                Env::prefixed(ENV_PREFIX)
                    .split("__")
                    .ignore(&NOT_CONFIGURATION),
            )
            .extract()
            .context("invalid configuration")
    }

    /// The issuer: the public URL without a trailing slash.
    pub fn issuer(&self) -> &str {
        self.public_url.as_str().trim_end_matches('/')
    }

    /// The protected resource MCP clients ask tokens for. `oauth-as` matches
    /// it exactly, so it is spelled one way only.
    pub fn resource(&self) -> String {
        format!("{}/mcp", self.issuer())
    }
}

/// The secrets, from the environment only.
pub struct Secrets {
    /// Our Simkl app's client secret (`SIMKL_MCP_CLIENT_SECRET`).
    pub simkl_client_secret: SecretString,
    /// The key Simkl tokens are sealed with (`SIMKL_MCP_VAULT_KEY`).
    pub vault_key: VaultKey,
}

impl Secrets {
    pub fn from_env() -> anyhow::Result<Secrets> {
        let read = |name: &str| {
            env::var(format!("{ENV_PREFIX}{}", name.to_uppercase()))
                .with_context(|| format!("{ENV_PREFIX}{} is not set", name.to_uppercase()))
        };
        Ok(Secrets {
            simkl_client_secret: SecretString::from(read("client_secret")?),
            vault_key: read("vault_key")?
                .parse()
                .map_err(|error| anyhow::anyhow!("{ENV_PREFIX}VAULT_KEY: {error}"))?,
        })
    }
}
