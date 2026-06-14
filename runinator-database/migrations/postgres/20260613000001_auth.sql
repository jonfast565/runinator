-- users, pluggable identities (local password now, oidc later), api keys, and refresh sessions.
CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    email TEXT NULL,
    is_admin BOOLEAN NOT NULL,
    disabled BOOLEAN NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS user_identities (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL,
    provider TEXT NOT NULL,
    subject TEXT NOT NULL,
    password_hash TEXT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE (provider, subject)
);
CREATE INDEX IF NOT EXISTS idx_user_identities_user ON user_identities(user_id);

CREATE TABLE IF NOT EXISTS api_keys (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    user_id UUID NULL,
    is_service BOOLEAN NOT NULL,
    is_admin BOOLEAN NOT NULL,
    key_prefix TEXT NOT NULL UNIQUE,
    key_hash TEXT NOT NULL,
    last_used_at BIGINT NULL,
    expires_at BIGINT NULL,
    disabled BOOLEAN NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS auth_sessions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL,
    refresh_token_hash TEXT NOT NULL UNIQUE,
    expires_at BIGINT NOT NULL,
    revoked BOOLEAN NOT NULL,
    created_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_auth_sessions_user ON auth_sessions(user_id);
