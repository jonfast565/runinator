-- users, pluggable identities (local password now, oidc later), api keys, and refresh sessions.
CREATE TABLE IF NOT EXISTS users (
    id BINARY(16) PRIMARY KEY,
    username VARCHAR(255) NOT NULL UNIQUE,
    email TEXT NULL,
    is_admin TINYINT(1) NOT NULL,
    disabled TINYINT(1) NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS user_identities (
    id BINARY(16) PRIMARY KEY,
    user_id BINARY(16) NOT NULL,
    provider VARCHAR(64) NOT NULL,
    subject VARCHAR(255) NOT NULL,
    password_hash TEXT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE (provider, subject)
);
CREATE INDEX idx_user_identities_user ON user_identities(user_id);

CREATE TABLE IF NOT EXISTS api_keys (
    id BINARY(16) PRIMARY KEY,
    name TEXT NOT NULL,
    user_id BINARY(16) NULL,
    is_service TINYINT(1) NOT NULL,
    is_admin TINYINT(1) NOT NULL,
    key_prefix VARCHAR(64) NOT NULL UNIQUE,
    key_hash TEXT NOT NULL,
    last_used_at BIGINT NULL,
    expires_at BIGINT NULL,
    disabled TINYINT(1) NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS auth_sessions (
    id BINARY(16) PRIMARY KEY,
    user_id BINARY(16) NOT NULL,
    refresh_token_hash VARCHAR(128) NOT NULL UNIQUE,
    expires_at BIGINT NOT NULL,
    revoked TINYINT(1) NOT NULL,
    created_at BIGINT NOT NULL
);
CREATE INDEX idx_auth_sessions_user ON auth_sessions(user_id);
