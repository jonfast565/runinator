CREATE TABLE IF NOT EXISTS execution_profiles (
    id BINARY(16) PRIMARY KEY NOT NULL,
    org_id BINARY(16) NULL,
    name VARCHAR(255) NOT NULL,
    description TEXT NOT NULL,
    credential_scopes TEXT NOT NULL,
    collection_json LONGTEXT NOT NULL,
    exposure_json LONGTEXT NOT NULL,
    config_version BIGINT NOT NULL,
    config_digest VARCHAR(64) NOT NULL,
    enabled BOOLEAN NOT NULL,
    current_revision BIGINT NULL,
    current_digest VARCHAR(64) NULL,
    current_publisher_id BINARY(16) NULL,
    published_at BIGINT NULL,
    expires_at BIGINT NULL,
    refresh_requested_at BIGINT NULL,
    health VARCHAR(32) NOT NULL,
    last_error TEXT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    UNIQUE KEY idx_execution_profiles_org_name (org_id, name)
);

CREATE TABLE IF NOT EXISTS execution_profile_revisions (
    profile_id BINARY(16) NOT NULL,
    revision BIGINT NOT NULL,
    digest VARCHAR(64) NOT NULL,
    size_bytes BIGINT NOT NULL,
    publisher_id BINARY(16) NULL,
    expires_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    uri TEXT NOT NULL,
    PRIMARY KEY (profile_id, revision),
    CONSTRAINT fk_execution_profile_revision_profile FOREIGN KEY (profile_id)
        REFERENCES execution_profiles(id) ON DELETE CASCADE
);
