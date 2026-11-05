//! Each Simkl connection's tokens, sealed at rest with the server's vault key.
//!
//! A connection is one Simkl sign-in, and its id (`conn_…`) is the subject of
//! the authorization server's tokens for that grant. The vault is the only
//! thing that refreshes Simkl tokens: a refresh kills the previous access
//! token at once, so refreshes of one connection are serialised by an
//! in-process lock (one replica), and never run inside a database
//! transaction (the store shares the one connection). Tokens are refreshed
//! lazily, when used within `refresh_margin` of expiry. The schema is
//! `migrations/0002_connections.sql`.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use aes_gcm::aead::{Aead, Generate, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use jiff::{SignedDuration, Timestamp};
use moka::future::Cache;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use simkl_api::AccessToken;
use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::auth::upstream::{SimklGrant, Upstream, UpstreamError};

/// Which key sealed a row. It is stored in the row's `key_id` column and bound
/// into the row's associated data. There is a single vault key, so it is
/// always 1. The column exists so a key ring can come later without a
/// migration: each key gets its own id, and rows still sealed under an old
/// one are re-sealed under the current one. Until then, rotating the key
/// leaves every row `Unsealable`, which signs everyone out.
const KEY_ID: i64 = 1;
/// A refresh token lives this long after its last use. This is Simkl's limit
/// (https://api.simkl.org/api-reference/auth-v2), not a setting.
const REFRESH_WINDOW: SignedDuration = SignedDuration::from_hours(180 * 24);

/// How the vault refreshes and prunes connections: the `auth` section of the
/// server's configuration. Durations take jiff's friendly form, such as `24h`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct VaultConfig {
    /// Refresh an access token used this close to its 7-day expiry.
    pub refresh_margin: SignedDuration,
    /// How long a new connection may go without tokens: from the Simkl
    /// callback to the client redeeming its authorization code (one minute in
    /// `oauth-as`).
    pub prune_grace: SignedDuration,
}

impl Default for VaultConfig {
    fn default() -> VaultConfig {
        VaultConfig {
            refresh_margin: SignedDuration::from_hours(24),
            prune_grace: SignedDuration::from_mins(10),
        }
    }
}

/// The AES-256-GCM key Simkl tokens are sealed with, parsed from 32 bytes in
/// standard base64 (`SIMKL_MCP_VAULT_KEY`).
pub struct VaultKey(Aes256Gcm);

impl FromStr for VaultKey {
    type Err = &'static str;

    fn from_str(encoded: &str) -> Result<VaultKey, &'static str> {
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| "the vault key is not base64")?;
        Aes256Gcm::new_from_slice(&bytes)
            .map(VaultKey)
            .map_err(|_| "the vault key is not 32 bytes")
    }
}

/// A Simkl connection, and the subject of its grant's tokens.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConnectionId(String);

impl ConnectionId {
    fn new_random() -> ConnectionId {
        ConnectionId(format!(
            "conn_{}",
            URL_SAFE_NO_PAD.encode(rand::random::<[u8; 16]>())
        ))
    }

    /// The connection an authorization-server token names as its subject.
    pub fn from_subject(subject: &str) -> ConnectionId {
        ConnectionId(subject.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Simkl tokens by connection.
pub struct Vault {
    pool: SqlitePool,
    key: VaultKey,
    upstream: Upstream,
    config: VaultConfig,
    /// One lock per connection being refreshed. Idle locks are dropped; a
    /// refresh holds its lock for seconds, far less than the idle time.
    refreshing: Cache<ConnectionId, Arc<Mutex<()>>>,
}

impl Vault {
    /// The vault over the pool `auth::open_database` returned.
    pub fn new(pool: SqlitePool, key: VaultKey, upstream: Upstream, config: VaultConfig) -> Vault {
        Vault {
            pool,
            key,
            upstream,
            config,
            refreshing: Cache::builder()
                .time_to_idle(Duration::from_secs(10 * 60))
                .build(),
        }
    }

    /// Keeps a new sign-in's tokens.
    pub async fn connect(&self, grant: SimklGrant) -> Result<ConnectionId, VaultError> {
        let id = ConnectionId::new_random();
        let now = Timestamp::now();
        sqlx::query(
            "INSERT INTO connections
                 (id, access_expires_at_ns, refresh_expires_at_ns, key_id, sealed, created_at_ns)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(nanos(grant.access_expires_at))
        .bind(nanos(now + REFRESH_WINDOW))
        .bind(KEY_ID)
        .bind(seal(
            &self.key,
            &id,
            &grant.access_token,
            &grant.refresh_token,
        ))
        .bind(nanos(now))
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    /// The connection's access token, refreshed first if it expires within
    /// `refresh_margin`.
    pub async fn access_token(&self, id: &ConnectionId) -> Result<AccessToken, VaultError> {
        let tokens = self.read(id).await?;
        if tokens.is_fresh(Timestamp::now(), self.config.refresh_margin) {
            return Ok(tokens.access_token);
        }
        let lock = self.lock(id).await;
        let _refreshing = lock.lock().await;
        let tokens = self.read(id).await?;
        if tokens.is_fresh(Timestamp::now(), self.config.refresh_margin) {
            return Ok(tokens.access_token);
        }
        self.refresh(id, &tokens).await
    }

    /// Ends connections the authorization server no longer has tokens for
    /// (revoked, reused, swept, or their client deleted), revoking them at
    /// Simkl, and forgets those whose refresh token has expired. A revocation
    /// Simkl fails is retried on the next prune. Returns how many ended.
    pub async fn prune(&self, now: Timestamp) -> Result<u64, VaultError> {
        let mut ended = 0;
        for connection in prunable(&self.pool, now, self.config.prune_grace).await? {
            if connection.refresh_expires_at > now {
                let (_, refresh_token) = open(&self.key, &connection.id, &connection.sealed)?;
                if let Err(error) = self.upstream.revoke(&refresh_token).await {
                    tracing::warn!(connection = connection.id.as_str(), %error, "revoking at Simkl failed");
                    continue;
                }
            }
            self.delete(&connection.id).await?;
            ended += 1;
        }
        Ok(ended)
    }

    async fn lock(&self, id: &ConnectionId) -> Arc<Mutex<()>> {
        self.refreshing
            .get_with_by_ref(id, async { Arc::new(Mutex::new(())) })
            .await
    }

    async fn read(&self, id: &ConnectionId) -> Result<Tokens, VaultError> {
        let row: Option<(i64, i64, Vec<u8>)> = sqlx::query_as(
            "SELECT access_expires_at_ns, key_id, sealed FROM connections WHERE id = ?",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await?;
        let (access_expires_at_ns, key_id, sealed) = row.ok_or(VaultError::Disconnected)?;
        if key_id != KEY_ID {
            return Err(VaultError::Unsealable);
        }
        let (access_token, refresh_token) = open(&self.key, id, &sealed)?;
        Ok(Tokens {
            access_token,
            refresh_token,
            access_expires_at: timestamp(access_expires_at_ns),
        })
    }

    /// Refreshes at Simkl and keeps the new access token. A refresh token
    /// Simkl rejects ends the connection.
    async fn refresh(&self, id: &ConnectionId, tokens: &Tokens) -> Result<AccessToken, VaultError> {
        let grant = match self.upstream.refresh(&tokens.refresh_token).await {
            Ok(grant) => grant,
            Err(UpstreamError::Rejected) => {
                self.delete(id).await?;
                return Err(VaultError::Disconnected);
            }
            Err(error) => return Err(error.into()),
        };
        let updated = sqlx::query(
            "UPDATE connections
             SET access_expires_at_ns = ?, refresh_expires_at_ns = ?, key_id = ?, sealed = ?
             WHERE id = ?",
        )
        .bind(nanos(grant.access_expires_at))
        .bind(nanos(Timestamp::now() + REFRESH_WINDOW))
        .bind(KEY_ID)
        .bind(seal(
            &self.key,
            id,
            &grant.access_token,
            &grant.refresh_token,
        ))
        .bind(id.as_str())
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(VaultError::Disconnected);
        }
        Ok(grant.access_token)
    }

    async fn delete(&self, id: &ConnectionId) -> Result<(), VaultError> {
        sqlx::query("DELETE FROM connections WHERE id = ?")
            .bind(id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

/// Why the vault could not hand out a token.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    /// No such connection, or Simkl ended its grant: the user signs in again.
    #[error("the Simkl connection is gone")]
    Disconnected,
    /// The row does not open with this vault key.
    #[error("the Simkl tokens do not open with the vault key")]
    Unsealable,
    #[error(transparent)]
    Upstream(#[from] UpstreamError),
    #[error("vault storage: {0}")]
    Storage(#[from] sqlx::Error),
}

/// A connection's tokens, opened.
struct Tokens {
    access_token: AccessToken,
    refresh_token: SecretString,
    access_expires_at: Timestamp,
}

impl Tokens {
    fn is_fresh(&self, now: Timestamp, margin: SignedDuration) -> bool {
        self.access_expires_at - margin > now
    }
}

/// A connection [`prune`](Vault::prune) ends.
struct Prunable {
    id: ConnectionId,
    refresh_expires_at: Timestamp,
    sealed: Vec<u8>,
}

/// Connections past the grace period that no access or refresh token names
/// as its subject, and connections whose refresh token has expired.
async fn prunable(
    pool: &SqlitePool,
    now: Timestamp,
    grace: SignedDuration,
) -> Result<Vec<Prunable>, VaultError> {
    let rows: Vec<(String, i64, Vec<u8>)> = sqlx::query_as(
        "SELECT id, refresh_expires_at_ns, sealed FROM connections c
         WHERE refresh_expires_at_ns <= ?1
            OR (created_at_ns <= ?2
                AND NOT EXISTS (SELECT 1 FROM access_tokens WHERE subject = c.id)
                AND NOT EXISTS (SELECT 1 FROM refresh_tokens WHERE subject = c.id))",
    )
    .bind(nanos(now))
    .bind(nanos(now - grace))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, refresh_expires_at_ns, sealed)| Prunable {
            id: ConnectionId(id),
            refresh_expires_at: timestamp(refresh_expires_at_ns),
            sealed,
        })
        .collect())
}

/// The access and refresh token, sealed for the row `id` under [`KEY_ID`]:
/// nonce, then ciphertext.
fn seal(
    key: &VaultKey,
    id: &ConnectionId,
    access_token: &AccessToken,
    refresh_token: &SecretString,
) -> Vec<u8> {
    let plaintext = SecretString::from(format!(
        "{}\n{}",
        access_token.expose_secret(),
        refresh_token.expose_secret()
    ));
    let nonce = Nonce::generate();
    let ciphertext = key
        .0
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext.expose_secret().as_bytes(),
                aad: &associated_data(id),
            },
        )
        .unwrap();
    [nonce.as_slice(), &ciphertext].concat()
}

/// Opens what [`seal`] sealed for the same `id`: the access and refresh token.
fn open(
    key: &VaultKey,
    id: &ConnectionId,
    sealed: &[u8],
) -> Result<(AccessToken, SecretString), VaultError> {
    let (nonce, ciphertext) = sealed.split_at_checked(12).ok_or(VaultError::Unsealable)?;
    let nonce = Nonce::try_from(nonce).map_err(|_| VaultError::Unsealable)?;
    let plaintext = key
        .0
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad: &associated_data(id),
            },
        )
        .map_err(|_| VaultError::Unsealable)?;
    let plaintext =
        SecretString::from(String::from_utf8(plaintext).map_err(|_| VaultError::Unsealable)?);
    let (access_token, refresh_token) = plaintext
        .expose_secret()
        .split_once('\n')
        .ok_or(VaultError::Unsealable)?;
    Ok((
        SecretString::from(access_token),
        SecretString::from(refresh_token),
    ))
}

/// Binds a sealed value to its row and key: `id`, then [`KEY_ID`].
fn associated_data(id: &ConnectionId) -> Vec<u8> {
    [id.as_str().as_bytes(), &KEY_ID.to_be_bytes()].concat()
}

/// Nanoseconds since the Unix epoch; every instant until 2262 fits.
fn nanos(at: Timestamp) -> i64 {
    i64::try_from(at.as_nanosecond()).unwrap()
}

/// Every `i64` of nanoseconds is within `jiff`'s range.
fn timestamp(nanos: i64) -> Timestamp {
    Timestamp::from_nanosecond(i128::from(nanos)).unwrap()
}

#[cfg(test)]
#[path = "vault_test.rs"]
mod tests;
