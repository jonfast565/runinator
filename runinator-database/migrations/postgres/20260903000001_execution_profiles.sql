CREATE TABLE IF NOT EXISTS execution_profiles (
    id UUID PRIMARY KEY NOT NULL,
    org_id UUID NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    credential_scopes TEXT NOT NULL,
    collection_json TEXT NOT NULL,
    exposure_json TEXT NOT NULL,
    config_version BIGINT NOT NULL,
    config_digest TEXT NOT NULL,
    enabled BOOLEAN NOT NULL,
    current_revision BIGINT NULL,
    current_digest TEXT NULL,
    current_publisher_id UUID NULL,
    published_at BIGINT NULL,
    expires_at BIGINT NULL,
    refresh_requested_at BIGINT NULL,
    health TEXT NOT NULL,
    last_error TEXT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_execution_profiles_org_name
    ON execution_profiles(org_id, name);

CREATE TABLE IF NOT EXISTS execution_profile_revisions (
    profile_id UUID NOT NULL REFERENCES execution_profiles(id) ON DELETE CASCADE,
    revision BIGINT NOT NULL,
    digest TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    publisher_id UUID NULL,
    expires_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    uri TEXT NOT NULL,
    PRIMARY KEY (profile_id, revision)
);
