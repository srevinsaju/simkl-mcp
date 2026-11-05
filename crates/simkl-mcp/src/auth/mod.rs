//! The OAuth 2.1 authorization server MCP clients talk to, sitting in front of
//! Simkl's AUTH V2.
//!
//! The protocol is `oauth-as`: authorization code with PKCE, refresh rotation
//! with reuse detection, RFC 8707 audiences, metadata, Dynamic Client
//! Registration and Client ID Metadata Documents. This module supplies only
//! what that library leaves to its host: storage, signing in the user (via
//! Simkl), and keeping each grant's Simkl tokens.

mod clients;
mod login;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "used once `serve` mounts the authorization server"
    )
)]
mod store;
mod upstream;
mod vault;

use std::str::FromStr;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

/// Opens (or creates) the database at `url` and applies migrations. The
/// store and the vault share the one connection it holds.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "used once `serve` mounts the authorization server"
    )
)]
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
