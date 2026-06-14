-- resource-based authorization: teams (named principals) and resource grants.
CREATE TABLE IF NOT EXISTS teams (
    id BINARY(16) PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS team_members (
    team_id BINARY(16) NOT NULL,
    user_id BINARY(16) NOT NULL,
    PRIMARY KEY (team_id, user_id)
);
CREATE INDEX idx_team_members_user ON team_members(user_id);

CREATE TABLE IF NOT EXISTS resource_grants (
    id BINARY(16) PRIMARY KEY,
    resource_type VARCHAR(64) NOT NULL,
    resource_id BINARY(16) NOT NULL,
    principal_type VARCHAR(32) NOT NULL,
    principal_id BINARY(16) NOT NULL,
    permission VARCHAR(32) NOT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE (resource_type, resource_id, principal_type, principal_id)
);
CREATE INDEX idx_resource_grants_resource ON resource_grants(resource_type, resource_id);
CREATE INDEX idx_resource_grants_principal ON resource_grants(principal_type, principal_id);
