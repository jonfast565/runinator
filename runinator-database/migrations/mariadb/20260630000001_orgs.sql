-- organizations (tenants) and per-user memberships. a user belongs to many orgs, each with a role.
CREATE TABLE IF NOT EXISTS organizations (
    id BINARY(16) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(255) NOT NULL UNIQUE,
    disabled TINYINT(1) NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS org_memberships (
    org_id BINARY(16) NOT NULL,
    user_id BINARY(16) NOT NULL,
    role VARCHAR(32) NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (org_id, user_id)
);
CREATE INDEX idx_org_memberships_user ON org_memberships(user_id);
