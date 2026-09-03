-- Team names are tenant-local. A team can be owned by the platform or one organization, never
-- by a user or another team.
ALTER TABLE teams RENAME TO teams_legacy;
CREATE TABLE teams (
    id BLOB PRIMARY KEY,
    name TEXT NOT NULL,
    scope_kind TEXT NOT NULL,
    scope_id BLOB NULL,
    created_at INTEGER NOT NULL,
    CHECK (scope_kind IN ('platform', 'organization')),
    CHECK ((scope_kind = 'platform' AND scope_id IS NULL) OR (scope_kind = 'organization' AND scope_id IS NOT NULL))
);
INSERT INTO teams (id, name, scope_kind, scope_id, created_at)
SELECT id, name, scope_kind, scope_id, created_at FROM teams_legacy;
DROP TABLE teams_legacy;
CREATE UNIQUE INDEX idx_teams_platform_name ON teams(name) WHERE scope_kind = 'platform';
CREATE UNIQUE INDEX idx_teams_organization_name ON teams(scope_id, name) WHERE scope_kind = 'organization';
