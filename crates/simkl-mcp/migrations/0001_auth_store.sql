-- oauth_as::Storage over SQLite (src/auth/store.rs).
--
-- Each table keeps the whole record as JSON in `record`; the other columns
-- exist only for lookup, cascades and sweeping. Instants are nanoseconds
-- since the Unix epoch. Access tokens, refresh tokens and authorization
-- codes are keyed by the SHA-256 hex of the secret, and the secret is blanked
-- in `record`.

CREATE TABLE clients (
    client_id TEXT PRIMARY KEY NOT NULL,
    record TEXT NOT NULL
) STRICT;

CREATE TABLE authorization_codes (
    code_hash TEXT PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL,
    expires_at_ns INTEGER NOT NULL,
    record TEXT NOT NULL
) STRICT;
CREATE INDEX authorization_codes_client_id ON authorization_codes (client_id);
CREATE INDEX authorization_codes_expires_at_ns ON authorization_codes (expires_at_ns);

-- `user_code` is normalized, so its uniqueness is the user code index.
CREATE TABLE device_grants (
    device_code TEXT PRIMARY KEY NOT NULL,
    user_code TEXT NOT NULL UNIQUE,
    client_id TEXT NOT NULL,
    expires_at_ns INTEGER NOT NULL,
    record TEXT NOT NULL
) STRICT;
CREATE INDEX device_grants_client_id ON device_grants (client_id);
CREATE INDEX device_grants_expires_at_ns ON device_grants (expires_at_ns);

CREATE TABLE access_tokens (
    token_hash TEXT PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL,
    family_id TEXT,
    expires_at_ns INTEGER NOT NULL,
    record TEXT NOT NULL
) STRICT;
CREATE INDEX access_tokens_client_id ON access_tokens (client_id);
CREATE INDEX access_tokens_family_id ON access_tokens (family_id);
CREATE INDEX access_tokens_expires_at_ns ON access_tokens (expires_at_ns);

-- `subject` lets a family revocation find the grant's vault connection.
CREATE TABLE refresh_tokens (
    token_hash TEXT PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL,
    family_id TEXT NOT NULL,
    subject TEXT,
    expires_at_ns INTEGER,
    record TEXT NOT NULL
) STRICT;
CREATE INDEX refresh_tokens_client_id ON refresh_tokens (client_id);
CREATE INDEX refresh_tokens_family_id ON refresh_tokens (family_id);
CREATE INDEX refresh_tokens_expires_at_ns ON refresh_tokens (expires_at_ns);

-- A `client` barrier refuses grants established at or before `recorded_at_ns`
-- (or any grant while the client is absent); a `family` barrier refuses
-- unconditionally. Both are swept once `until_ns` passes.
CREATE TABLE revocation_barriers (
    kind TEXT NOT NULL CHECK (kind IN ('client', 'family')),
    scope_id TEXT NOT NULL,
    recorded_at_ns INTEGER NOT NULL,
    until_ns INTEGER NOT NULL,
    PRIMARY KEY (kind, scope_id)
) STRICT;
CREATE INDEX revocation_barriers_until_ns ON revocation_barriers (until_ns);
