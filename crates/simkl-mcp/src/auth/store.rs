//! `oauth_as::Storage` over SQLite, checked with the library's storage
//! conformance harness.
//!
//! Every method is one transaction on a single connection, which gives the
//! atomicity `take_*`, the compare-and-swaps and `rotate_refresh_token` need
//! without SQLite ever reporting a busy writer. Records are stored whole as
//! JSON (`RefreshTokenRecord::retry` cannot be built outside `oauth-as`); the
//! columns beside them exist for lookup, cascades and sweeping. Access tokens,
//! refresh tokens and authorization codes are keyed by their SHA-256 and
//! blanked in the stored JSON: every lookup is handed the secret, so it is
//! restored from the argument on the way out. The schema is
//! `migrations/0001_auth_store.sql`.

use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;

use oauth_as::device::normalize_user_code;
use oauth_as::{
    AuthorizationCodeRecord, AuthorizationCodeState, Client, ClientId, DeviceGrant,
    DeviceGrantState, IssuedToken, RefreshTokenRecord, RevocationWindow, SecretHash, Storage,
    StorageError, WriteOutcome,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{SqliteConnection, SqlitePool};

/// The authorization server's state: clients, codes, tokens and revocation
/// barriers, in one SQLite database.
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    /// Opens (or creates) the database at `url` and applies migrations.
    pub async fn open(url: &str) -> Result<Store, sqlx::Error> {
        let options = SqliteConnectOptions::from_str(url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        // One connection that never closes: transactions queue on it instead
        // of racing for the write lock, and `sqlite::memory:` keeps its data.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await?;
        sqlx::migrate!().run(&pool).await?;
        Ok(Store { pool })
    }

    async fn begin(&self) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>, StorageError> {
        self.pool.begin().await.map_err(db_error)
    }
}

impl Storage for Store {
    async fn get_client(&self, client_id: &ClientId) -> Result<Option<Arc<Client>>, StorageError> {
        let record: Option<String> =
            sqlx::query_scalar("SELECT record FROM clients WHERE client_id = ?")
                .bind(client_id.as_str())
                .fetch_optional(&self.pool)
                .await
                .map_err(db_error)?;
        record.map(|r| decode(&r).map(Arc::new)).transpose()
    }

    async fn put_client(&self, client: Client) -> Result<(), StorageError> {
        let mut conn = self.pool.acquire().await.map_err(db_error)?;
        write_client(&mut conn, &client).await
    }

    async fn compare_and_swap_client(
        &self,
        expected: &Client,
        updated: Client,
    ) -> Result<bool, StorageError> {
        let mut tx = self.begin().await?;
        let current: Option<String> =
            sqlx::query_scalar("SELECT record FROM clients WHERE client_id = ?")
                .bind(updated.client_id.as_str())
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_error)?;
        match current.map(|r| decode::<Client>(&r)).transpose()? {
            Some(current) if current == *expected => {}
            _ => return Ok(false),
        }
        write_client(&mut tx, &updated).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(true)
    }

    async fn delete_client(
        &self,
        client_id: &ClientId,
        window: RevocationWindow,
    ) -> Result<bool, StorageError> {
        reject_empty("client_id", client_id.as_str())?;
        let mut tx = self.begin().await?;
        let existed = sqlx::query("DELETE FROM clients WHERE client_id = ?")
            .bind(client_id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(db_error)?
            .rows_affected()
            > 0;
        record_barrier(&mut tx, Barrier::Client, client_id.as_str(), window).await?;
        for table in [
            "access_tokens",
            "refresh_tokens",
            "authorization_codes",
            "device_grants",
        ] {
            sqlx::query(&format!("DELETE FROM {table} WHERE client_id = ?"))
                .bind(client_id.as_str())
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(existed)
    }

    async fn put_device_grant(&self, grant: DeviceGrant) -> Result<(), StorageError> {
        let mut conn = self.pool.acquire().await.map_err(db_error)?;
        write_device_grant(&mut conn, &grant).await
    }

    async fn get_device_grant(
        &self,
        device_code: &str,
    ) -> Result<Option<DeviceGrant>, StorageError> {
        let record: Option<String> =
            sqlx::query_scalar("SELECT record FROM device_grants WHERE device_code = ?")
                .bind(device_code)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_error)?;
        record.map(|r| decode(&r)).transpose()
    }

    async fn find_device_grant_by_user_code(
        &self,
        normalized_user_code: &str,
    ) -> Result<Option<DeviceGrant>, StorageError> {
        let record: Option<String> =
            sqlx::query_scalar("SELECT record FROM device_grants WHERE user_code = ?")
                .bind(normalized_user_code)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_error)?;
        record.map(|r| decode(&r)).transpose()
    }

    async fn take_device_grant(
        &self,
        device_code: &str,
    ) -> Result<Option<DeviceGrant>, StorageError> {
        let record: Option<String> =
            sqlx::query_scalar("DELETE FROM device_grants WHERE device_code = ? RETURNING record")
                .bind(device_code)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_error)?;
        record.map(|r| decode(&r)).transpose()
    }

    async fn compare_and_swap_device_grant(
        &self,
        expected: &DeviceGrantState,
        updated: DeviceGrant,
    ) -> Result<bool, StorageError> {
        let mut tx = self.begin().await?;
        let current: Option<String> =
            sqlx::query_scalar("SELECT record FROM device_grants WHERE device_code = ?")
                .bind(&updated.device_code)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_error)?;
        match current.map(|r| decode::<DeviceGrant>(&r)).transpose()? {
            Some(current) if current.state == *expected => {}
            _ => return Ok(false),
        }
        write_device_grant(&mut tx, &updated).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(true)
    }

    async fn put_authorization_code(
        &self,
        record: AuthorizationCodeRecord,
    ) -> Result<(), StorageError> {
        let mut conn = self.pool.acquire().await.map_err(db_error)?;
        write_authorization_code(&mut conn, &record).await
    }

    async fn compare_and_swap_authorization_code(
        &self,
        expected: &AuthorizationCodeState,
        updated: AuthorizationCodeRecord,
    ) -> Result<bool, StorageError> {
        let mut tx = self.begin().await?;
        let current: Option<String> =
            sqlx::query_scalar("SELECT record FROM authorization_codes WHERE code_hash = ?")
                .bind(secret_hash(&updated.code))
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_error)?;
        match current
            .map(|r| decode::<AuthorizationCodeRecord>(&r))
            .transpose()?
        {
            Some(current) if current.state == *expected => {}
            _ => return Ok(false),
        }
        write_authorization_code(&mut tx, &updated).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(true)
    }

    async fn take_authorization_code(
        &self,
        code: &str,
    ) -> Result<Option<AuthorizationCodeRecord>, StorageError> {
        let record: Option<String> = sqlx::query_scalar(
            "DELETE FROM authorization_codes WHERE code_hash = ? RETURNING record",
        )
        .bind(secret_hash(code))
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        record
            .map(|r| {
                let mut record: AuthorizationCodeRecord = decode(&r)?;
                record.code = code.to_owned();
                Ok(record)
            })
            .transpose()
    }

    async fn put_token(&self, token: IssuedToken) -> Result<WriteOutcome, StorageError> {
        let mut tx = self.begin().await?;
        if is_revoked(
            &mut tx,
            &token.client_id,
            token.family_id.as_deref(),
            token.grant_established_at,
        )
        .await?
        {
            return Ok(WriteOutcome::RefusedRevoked);
        }
        write_token(&mut tx, &token).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(WriteOutcome::Applied)
    }

    async fn get_token(
        &self,
        access_token: &str,
    ) -> Result<Option<Arc<IssuedToken>>, StorageError> {
        let record: Option<String> =
            sqlx::query_scalar("SELECT record FROM access_tokens WHERE token_hash = ?")
                .bind(secret_hash(access_token))
                .fetch_optional(&self.pool)
                .await
                .map_err(db_error)?;
        record
            .map(|r| {
                let mut token: IssuedToken = decode(&r)?;
                token.access_token = access_token.to_owned();
                Ok(Arc::new(token))
            })
            .transpose()
    }

    async fn delete_token(&self, access_token: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM access_tokens WHERE token_hash = ?")
            .bind(secret_hash(access_token))
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
        Ok(())
    }

    async fn put_refresh_token(
        &self,
        record: RefreshTokenRecord,
    ) -> Result<WriteOutcome, StorageError> {
        let mut tx = self.begin().await?;
        if is_revoked(
            &mut tx,
            &record.client_id,
            Some(&record.family_id),
            record.grant_established_at,
        )
        .await?
        {
            return Ok(WriteOutcome::RefusedRevoked);
        }
        write_refresh_token(&mut tx, &record).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(WriteOutcome::Applied)
    }

    async fn get_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<Option<Arc<RefreshTokenRecord>>, StorageError> {
        let mut conn = self.pool.acquire().await.map_err(db_error)?;
        Ok(read_refresh_token(&mut conn, refresh_token)
            .await?
            .map(Arc::new))
    }

    async fn rotate_refresh_token(
        &self,
        expected: &RefreshTokenRecord,
        spent: &RefreshTokenRecord,
        access: &IssuedToken,
        next: Option<&RefreshTokenRecord>,
    ) -> Result<bool, StorageError> {
        let mut tx = self.begin().await?;
        if read_refresh_token(&mut tx, &expected.refresh_token)
            .await?
            .as_ref()
            != Some(expected)
            || is_revoked(
                &mut tx,
                &expected.client_id,
                Some(&expected.family_id),
                expected.grant_established_at,
            )
            .await?
        {
            return Ok(false);
        }
        write_refresh_token(&mut tx, spent).await?;
        write_token(&mut tx, access).await?;
        if let Some(next) = next {
            write_refresh_token(&mut tx, next).await?;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(true)
    }

    async fn take_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<Option<RefreshTokenRecord>, StorageError> {
        let record: Option<String> =
            sqlx::query_scalar("DELETE FROM refresh_tokens WHERE token_hash = ? RETURNING record")
                .bind(secret_hash(refresh_token))
                .fetch_optional(&self.pool)
                .await
                .map_err(db_error)?;
        record
            .map(|r| {
                let mut record: RefreshTokenRecord = decode(&r)?;
                record.refresh_token = refresh_token.to_owned();
                Ok(record)
            })
            .transpose()
    }

    async fn revoke_token_family(
        &self,
        family_id: &str,
        window: RevocationWindow,
    ) -> Result<u64, StorageError> {
        reject_empty("family_id", family_id)?;
        let mut tx = self.begin().await?;
        record_barrier(&mut tx, Barrier::Family, family_id, window).await?;
        let mut removed = 0;
        for table in ["access_tokens", "refresh_tokens"] {
            removed += sqlx::query(&format!("DELETE FROM {table} WHERE family_id = ?"))
                .bind(family_id)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?
                .rows_affected();
        }
        tx.commit().await.map_err(db_error)?;
        Ok(removed)
    }

    async fn sweep_expired(&self, now: SystemTime) -> Result<u64, StorageError> {
        let now = nanos(now)?;
        let mut tx = self.begin().await?;
        let mut removed = 0;
        for statement in [
            "DELETE FROM device_grants WHERE expires_at_ns <= ?",
            "DELETE FROM authorization_codes WHERE expires_at_ns <= ?",
            "DELETE FROM access_tokens WHERE expires_at_ns <= ?",
            "DELETE FROM refresh_tokens WHERE expires_at_ns <= ?",
            "DELETE FROM revocation_barriers WHERE until_ns <= ?",
        ] {
            removed += sqlx::query(statement)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?
                .rows_affected();
        }
        tx.commit().await.map_err(db_error)?;
        Ok(removed)
    }
}

/// What a revocation barrier refuses: a client's grants established up to
/// `recorded_at`, or every token of a refresh family.
#[derive(Clone, Copy)]
enum Barrier {
    Client,
    Family,
}

impl Barrier {
    fn kind(self) -> &'static str {
        match self {
            Barrier::Client => "client",
            Barrier::Family => "family",
        }
    }
}

/// Records a barrier, keeping the later of each instant when the same scope is
/// revoked again.
async fn record_barrier(
    conn: &mut SqliteConnection,
    barrier: Barrier,
    scope_id: &str,
    window: RevocationWindow,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO revocation_barriers (kind, scope_id, recorded_at_ns, until_ns)
         VALUES (?, ?, ?, ?)
         ON CONFLICT (kind, scope_id) DO UPDATE SET
             recorded_at_ns = max(recorded_at_ns, excluded.recorded_at_ns),
             until_ns = max(until_ns, excluded.until_ns)",
    )
    .bind(barrier.kind())
    .bind(scope_id)
    .bind(nanos(window.recorded_at)?)
    .bind(nanos(window.until)?)
    .execute(conn)
    .await
    .map_err(db_error)?;
    Ok(())
}

/// Whether a barrier refuses a token for this grant. A client barrier also
/// refuses while the client is absent.
async fn is_revoked(
    conn: &mut SqliteConnection,
    client_id: &ClientId,
    family_id: Option<&str>,
    grant_established_at: SystemTime,
) -> Result<bool, StorageError> {
    sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM revocation_barriers
             WHERE (kind = 'client' AND scope_id = ?1 AND (
                        recorded_at_ns >= ?2
                        OR NOT EXISTS (SELECT 1 FROM clients WHERE client_id = ?1)))
                OR (kind = 'family' AND scope_id = ?3)
         )",
    )
    .bind(client_id.as_str())
    .bind(nanos(grant_established_at)?)
    .bind(family_id)
    .fetch_one(conn)
    .await
    .map_err(db_error)
}

async fn write_client(conn: &mut SqliteConnection, client: &Client) -> Result<(), StorageError> {
    sqlx::query("INSERT OR REPLACE INTO clients (client_id, record) VALUES (?, ?)")
        .bind(client.client_id.as_str())
        .bind(encode(client)?)
        .execute(conn)
        .await
        .map_err(db_error)?;
    Ok(())
}

/// Upserts by device code. A user code already held by another device code
/// fails the `UNIQUE` constraint, so nothing is written.
async fn write_device_grant(
    conn: &mut SqliteConnection,
    grant: &DeviceGrant,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO device_grants (device_code, user_code, client_id, expires_at_ns, record)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (device_code) DO UPDATE SET
             user_code = excluded.user_code,
             client_id = excluded.client_id,
             expires_at_ns = excluded.expires_at_ns,
             record = excluded.record",
    )
    .bind(&grant.device_code)
    .bind(normalize_user_code(&grant.user_code))
    .bind(grant.client_id.as_str())
    .bind(nanos(grant.expires_at)?)
    .bind(encode(grant)?)
    .execute(conn)
    .await
    .map_err(db_error)?;
    Ok(())
}

async fn write_authorization_code(
    conn: &mut SqliteConnection,
    record: &AuthorizationCodeRecord,
) -> Result<(), StorageError> {
    let mut stored = record.clone();
    stored.code = String::new();
    sqlx::query(
        "INSERT OR REPLACE INTO authorization_codes (code_hash, client_id, expires_at_ns, record)
         VALUES (?, ?, ?, ?)",
    )
    .bind(secret_hash(&record.code))
    .bind(record.client_id.as_str())
    .bind(nanos(record.expires_at)?)
    .bind(encode(&stored)?)
    .execute(conn)
    .await
    .map_err(db_error)?;
    Ok(())
}

async fn write_token(conn: &mut SqliteConnection, token: &IssuedToken) -> Result<(), StorageError> {
    let mut stored = token.clone();
    stored.access_token = String::new();
    sqlx::query(
        "INSERT OR REPLACE INTO access_tokens (token_hash, client_id, family_id, expires_at_ns, record)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(secret_hash(&token.access_token))
    .bind(token.client_id.as_str())
    .bind(token.family_id.as_deref())
    .bind(nanos(token.expires_at)?)
    .bind(encode(&stored)?)
    .execute(conn)
    .await
    .map_err(db_error)?;
    Ok(())
}

async fn write_refresh_token(
    conn: &mut SqliteConnection,
    record: &RefreshTokenRecord,
) -> Result<(), StorageError> {
    let mut stored = record.clone();
    stored.refresh_token = String::new();
    sqlx::query(
        "INSERT OR REPLACE INTO refresh_tokens
             (token_hash, client_id, family_id, subject, expires_at_ns, record)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(secret_hash(&record.refresh_token))
    .bind(record.client_id.as_str())
    .bind(&record.family_id)
    .bind(record.subject.as_deref())
    .bind(record.expires_at.map(nanos).transpose()?)
    .bind(encode(&stored)?)
    .execute(conn)
    .await
    .map_err(db_error)?;
    Ok(())
}

async fn read_refresh_token(
    conn: &mut SqliteConnection,
    refresh_token: &str,
) -> Result<Option<RefreshTokenRecord>, StorageError> {
    let record: Option<String> =
        sqlx::query_scalar("SELECT record FROM refresh_tokens WHERE token_hash = ?")
            .bind(secret_hash(refresh_token))
            .fetch_optional(conn)
            .await
            .map_err(db_error)?;
    record
        .map(|r| {
            let mut record: RefreshTokenRecord = decode(&r)?;
            record.refresh_token = refresh_token.to_owned();
            Ok(record)
        })
        .transpose()
}

fn reject_empty(what: &str, id: &str) -> Result<(), StorageError> {
    if id.is_empty() {
        return Err(StorageError::new(format!(
            "a revocation needs a non-empty {what}"
        )));
    }
    Ok(())
}

fn secret_hash(secret: &str) -> String {
    SecretHash::sha256(secret).encoded().to_owned()
}

/// Nanoseconds since the Unix epoch. Barriers compare grant instants with
/// `<=`, so whole seconds would refuse grants made in the same second.
fn nanos(at: SystemTime) -> Result<i64, StorageError> {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_nanos()).ok())
        .ok_or_else(|| StorageError::new("instant is outside the storable range"))
}

fn encode<T: Serialize>(record: &T) -> Result<String, StorageError> {
    serde_json::to_string(record).map_err(json_error)
}

fn decode<T: DeserializeOwned>(record: &str) -> Result<T, StorageError> {
    serde_json::from_str(record).map_err(json_error)
}

/// Names the failure without serde's message, which can quote stored values
/// (codes keep the tokens they issued).
fn json_error(e: serde_json::Error) -> StorageError {
    StorageError::new(format!(
        "stored record is not valid JSON for its type ({:?} at line {}, column {})",
        e.classify(),
        e.line(),
        e.column()
    ))
}

fn db_error(e: sqlx::Error) -> StorageError {
    StorageError::new(e.to_string())
}

#[cfg(test)]
#[path = "store_test.rs"]
mod tests;
