-- users, pluggable identities (local password now, oidc later), api keys, and refresh sessions.
CREATE TABLE IF NOT EXISTS users (
    id BLOB PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    email TEXT NULL,
    is_admin BOOL NOT NULL,
    disabled BOOL NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS user_identities (
    id BLOB PRIMARY KEY,
    user_id BLOB NOT NULL,
    provider TEXT NOT NULL,
    subject TEXT NOT NULL,
    password_hash TEXT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (provider, subject)
);
CREATE INDEX IF NOT EXISTS idx_user_identities_user ON user_identities(user_id);

CREATE TABLE IF NOT EXISTS api_keys (
    id BLOB PRIMARY KEY,
    name TEXT NOT NULL,
    user_id BLOB NULL,
    is_service BOOL NOT NULL,
    is_admin BOOL NOT NULL,
    key_prefix TEXT NOT NULL UNIQUE,
    key_hash TEXT NOT NULL,
    last_used_at INTEGER NULL,
    expires_at INTEGER NULL,
    disabled BOOL NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS auth_sessions (
    id BLOB PRIMARY KEY,
    user_id BLOB NOT NULL,
    refresh_token_hash TEXT NOT NULL UNIQUE,
    expires_at INTEGER NOT NULL,
    revoked BOOL NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_auth_sessions_user ON auth_sessions(user_id);
