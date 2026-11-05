-- Simkl connections (src/auth/vault.rs): one row per sign-in, and the
-- subject of the authorization server's tokens for that grant.
--
-- `sealed` is the access and refresh token, sealed with AES-256-GCM under the
-- vault key `key_id` (always 1 until keys rotate): a 96-bit nonce, then the
-- ciphertext. The associated data is `id` followed by `key_id`, so a sealed
-- value only opens in its own row. Instants are nanoseconds since the Unix
-- epoch; Simkl's refresh token lives 180 days from its last use.
CREATE TABLE connections (
    id TEXT PRIMARY KEY NOT NULL,
    access_expires_at_ns INTEGER NOT NULL,
    refresh_expires_at_ns INTEGER NOT NULL,
    key_id INTEGER NOT NULL,
    sealed BLOB NOT NULL,
    created_at_ns INTEGER NOT NULL
) STRICT;
CREATE INDEX connections_refresh_expires_at_ns ON connections (refresh_expires_at_ns);

-- So the vault can tell which connections still have tokens.
ALTER TABLE access_tokens ADD COLUMN subject TEXT;
CREATE INDEX access_tokens_subject ON access_tokens (subject);
CREATE INDEX refresh_tokens_subject ON refresh_tokens (subject);
