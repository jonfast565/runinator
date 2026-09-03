CREATE TABLE IF NOT EXISTS execution_profiles (
    id BLOB PRIMARY KEY NOT NULL,
    org_id BLOB NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    credential_scopes TEXT NOT NULL,
    collection_json TEXT NOT NULL,
    exposure_json TEXT NOT NULL,
    config_version INTEGER NOT NULL,
    config_digest TEXT NOT NULL,
    enabled BOOLEAN NOT NULL,
    current_revision INTEGER NULL,
    current_digest TEXT NULL,
    current_publisher_id BLOB NULL,
    published_at INTEGER NULL,
    expires_at INTEGER NULL,
    refresh_requested_at INTEGER NULL,
    health TEXT NOT NULL,
    last_error TEXT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_execution_profiles_org_name
    ON execution_profiles(org_id, name);

CREATE TABLE IF NOT EXISTS execution_profile_revisions (
    profile_id BLOB NOT NULL REFERENCES execution_profiles(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL,
    digest TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    publisher_id BLOB NULL,
    expires_at INTEGER NULL,
    created_at INTEGER NOT NULL,
    uri TEXT NOT NULL,
    PRIMARY KEY (profile_id, revision)
);
