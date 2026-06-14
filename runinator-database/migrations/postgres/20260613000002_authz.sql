-- resource-based authorization: teams (named principals) and resource grants.
CREATE TABLE IF NOT EXISTS teams (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS team_members (
    team_id UUID NOT NULL,
    user_id UUID NOT NULL,
    PRIMARY KEY (team_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_team_members_user ON team_members(user_id);

CREATE TABLE IF NOT EXISTS resource_grants (
    id UUID PRIMARY KEY,
    resource_type TEXT NOT NULL,
    resource_id UUID NOT NULL,
    principal_type TEXT NOT NULL,
    principal_id UUID NOT NULL,
    permission TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE (resource_type, resource_id, principal_type, principal_id)
);
CREATE INDEX IF NOT EXISTS idx_resource_grants_resource ON resource_grants(resource_type, resource_id);
CREATE INDEX IF NOT EXISTS idx_resource_grants_principal ON resource_grants(principal_type, principal_id);
