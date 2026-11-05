use std::collections::BTreeSet;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::SqlitePool;

use super::{ConnectionId, VaultConfig, VaultError, VaultKey, nanos, open, prunable, seal};
use crate::auth::open_database;

#[test]
fn sealed_tokens_open_only_in_their_own_row() {
    let key: VaultKey = STANDARD.encode([7u8; 32]).parse().unwrap();
    let id = ConnectionId::new_random();
    let sealed = seal(
        &key,
        &id,
        &SecretString::from("simkl_at_access"),
        &SecretString::from("simkl_rt_refresh"),
    );

    let (access_token, refresh_token) = open(&key, &id, &sealed).unwrap();
    assert_eq!(access_token.expose_secret(), "simkl_at_access");
    assert_eq!(refresh_token.expose_secret(), "simkl_rt_refresh");

    let other = ConnectionId::new_random();
    assert!(matches!(
        open(&key, &other, &sealed),
        Err(VaultError::Unsealable)
    ));
}

async fn insert_connection(
    pool: &SqlitePool,
    id: &str,
    created_at: Timestamp,
    refresh_expires_at: Timestamp,
) {
    sqlx::query(
        "INSERT INTO connections
             (id, access_expires_at_ns, refresh_expires_at_ns, key_id, sealed, created_at_ns)
         VALUES (?, 0, ?, 1, x'', ?)",
    )
    .bind(id)
    .bind(nanos(refresh_expires_at))
    .bind(nanos(created_at))
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn prunes_connections_without_tokens_or_past_their_refresh_window() {
    let pool = open_database("sqlite::memory:").await.unwrap();
    let now = Timestamp::now();
    let long_ago = now - SignedDuration::from_hours(1);
    let later = now + SignedDuration::from_hours(24);

    insert_connection(&pool, "conn_with_access_token", long_ago, later).await;
    insert_connection(&pool, "conn_with_refresh_token", long_ago, later).await;
    insert_connection(&pool, "conn_orphaned", long_ago, later).await;
    insert_connection(&pool, "conn_just_signed_in", now, later).await;
    insert_connection(&pool, "conn_refresh_expired", long_ago, now).await;
    sqlx::query(
        "INSERT INTO access_tokens (token_hash, client_id, family_id, subject, expires_at_ns, record)
         VALUES ('a', 'client', 'family', 'conn_with_access_token', 0, '{}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO refresh_tokens (token_hash, client_id, family_id, subject, expires_at_ns, record)
         VALUES ('r', 'client', 'family', 'conn_with_refresh_token', NULL, '{}'),
                ('s', 'client', 'family', 'conn_refresh_expired', NULL, '{}')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let pruned: BTreeSet<String> = prunable(&pool, now, VaultConfig::default().prune_grace)
        .await
        .unwrap()
        .into_iter()
        .map(|connection| connection.id.as_str().to_owned())
        .collect();
    assert_eq!(
        pruned,
        BTreeSet::from([
            "conn_orphaned".to_owned(),
            "conn_refresh_expired".to_owned()
        ])
    );
}
