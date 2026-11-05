//! The OAuth 2.1 authorization server MCP clients talk to, sitting in front of
//! Simkl's AUTH V2.
//!
//! The protocol is `oauth-as`: authorization code with PKCE, refresh rotation
//! with reuse detection, RFC 8707 audiences, metadata and Dynamic Client
//! Registration. This module supplies only what that library leaves to its
//! host: storage, signing in the user (via Simkl), keeping each grant's Simkl
//! tokens, and the sweeper.

mod clients;
mod login;
mod store;
mod upstream;
mod vault;

use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;

use axum::Router;
use axum::routing::get;
use jiff::{SignedDuration, Timestamp};
use oauth_as::http::ServiceBuilder;
use oauth_as::registration::RegistrationConfig;
use oauth_as::server::{AuthorizationServer, ServerConfig};
use oauth_as::store::Storage;
use secrecy::SecretString;
use serde::Deserialize;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use url::Url;

pub use clients::{READ_SCOPE, WRITE_SCOPE};
pub use vault::{ConnectionId, Vault, VaultConfig, VaultError, VaultKey};

use clients::{OpenRegistration, all_scopes};
use login::Login;
use store::Store;
use upstream::Upstream;

/// The path Simkl redirects back to. It is registered with Simkl exactly, so
/// it never changes without re-registering.
pub const SIMKL_CALLBACK_PATH: &str = "/auth/simkl/callback";

/// The `auth` section of the server's configuration. Durations take jiff's
/// friendly form, such as `60s`. Unknown keys are an error, so a misspelled
/// setting never silently keeps its default.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthConfig {
    /// Refresh a Simkl access token used this close to its 7-day expiry.
    pub refresh_margin: SignedDuration,
    /// How long a new connection may go without tokens, from the Simkl
    /// callback to the client redeeming its authorization code.
    pub prune_grace: SignedDuration,
    /// How often expired grants are swept and orphaned connections pruned.
    pub sweep_interval: SignedDuration,
    /// How long a sign-in may take, consent page to Simkl callback.
    pub sign_in_ttl: SignedDuration,
    /// How long a token or revoke request to Simkl may take.
    pub simkl_timeout: SignedDuration,
}

impl Default for AuthConfig {
    fn default() -> AuthConfig {
        let vault = VaultConfig::default();
        AuthConfig {
            refresh_margin: vault.refresh_margin,
            prune_grace: vault.prune_grace,
            sweep_interval: SignedDuration::from_secs(60),
            sign_in_ttl: SignedDuration::from_mins(10),
            simkl_timeout: SignedDuration::from_secs(30),
        }
    }
}

impl AuthConfig {
    fn vault(&self) -> VaultConfig {
        VaultConfig {
            refresh_margin: self.refresh_margin,
            prune_grace: self.prune_grace,
        }
    }
}

/// The authorization server, the vault, and the sign-in routes over them.
pub struct Auth {
    server: Arc<AuthorizationServer<Store>>,
    vault: Arc<Vault>,
    login: Login,
    sweep_interval: SignedDuration,
}

impl Auth {
    /// The authorization server at `issuer`, for the one protected resource
    /// `resource` (the `/mcp` URL), over the database at `database`.
    pub async fn open(
        issuer: &Url,
        resource: &str,
        database: &str,
        simkl_client_id: String,
        simkl_client_secret: SecretString,
        vault_key: VaultKey,
        config: AuthConfig,
    ) -> anyhow::Result<Auth> {
        let issuer = issuer.as_str().trim_end_matches('/').to_owned();
        let pool = open_database(database).await?;

        let mut registration = RegistrationConfig::new();
        registration.allowed_scopes = all_scopes();
        registration.management_enabled = false;
        // The device flow is advertised by `oauth-as` regardless, but no
        // registered client may use it: they get the code and refresh grants.
        let mut server_config = ServerConfig::new(issuer.clone(), format!("{issuer}/device"));
        server_config.registration = Some(Box::new(registration));
        server_config.scopes_supported = Some(vec![READ_SCOPE.to_owned(), WRITE_SCOPE.to_owned()]);
        server_config.protected_resources = Some(vec![resource.to_owned()]);
        server_config.allowed_resources = Some(Box::new([resource.into()]));
        let server = Arc::new(
            AuthorizationServer::new(server_config, Store::new(pool.clone()))
                .with_registration_policy(Box::new(OpenRegistration)),
        );

        let callback = Url::parse(&format!("{issuer}{SIMKL_CALLBACK_PATH}"))?;
        let upstream = Upstream::new(
            simkl_client_id,
            simkl_client_secret,
            callback,
            config.simkl_timeout.unsigned_abs(),
        )?;
        let vault = Arc::new(Vault::new(
            pool,
            vault_key,
            upstream.clone(),
            config.vault(),
        ));
        let secure = issuer.starts_with("https://");
        let login = Login::new(
            server.clone(),
            upstream,
            vault.clone(),
            secure,
            config.sign_in_ttl.unsigned_abs(),
        );
        Ok(Auth {
            server,
            vault,
            login,
            sweep_interval: config.sweep_interval,
        })
    }

    pub fn server(&self) -> &Arc<AuthorizationServer<Store>> {
        &self.server
    }

    pub fn vault(&self) -> &Arc<Vault> {
        &self.vault
    }

    /// Our sign-in routes, with every other OAuth endpoint (metadata, token,
    /// registration, revocation) answered by `oauth-as`.
    pub fn router(&self) -> anyhow::Result<Router> {
        let oauth = ServiceBuilder::new(self.server.clone()).build()?;
        let login = Router::new()
            .route("/authorize", get(login::authorize).post(login::consent))
            .route(SIMKL_CALLBACK_PATH, get(login::callback))
            .with_state(self.login.clone());
        Ok(login.merge(Router::from(oauth)))
    }

    /// Nothing in `oauth-as` runs on a timer, so this sweeps expired grants
    /// and, after them, prunes connections no token names any more.
    pub fn spawn_sweeper(&self) {
        let server = self.server.clone();
        let vault = self.vault.clone();
        let interval = self.sweep_interval.unsigned_abs();
        tokio::spawn(async move {
            loop {
                if let Err(error) = server.store().sweep_expired(SystemTime::now()).await {
                    tracing::warn!(%error, "sweeping expired grants failed");
                }
                if let Err(error) = vault.prune(Timestamp::now()).await {
                    tracing::warn!(%error, "pruning Simkl connections failed");
                }
                tokio::time::sleep(interval).await;
            }
        });
    }
}

/// Opens (or creates) the database at `url` and applies migrations. The
/// store and the vault share the one connection it holds.
pub async fn open_database(url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);
    // One connection that never closes: transactions queue on it instead of
    // racing for the write lock, and `sqlite::memory:` keeps its data.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(options)
        .await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}
