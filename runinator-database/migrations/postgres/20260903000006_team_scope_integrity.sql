-- Team names are tenant-local. A team can be owned by the platform or one organization, never
-- by a user or another team.
ALTER TABLE teams DROP CONSTRAINT IF EXISTS teams_name_key;
ALTER TABLE teams
    ADD CONSTRAINT teams_scope_check
    CHECK ((scope_kind = 'platform' AND scope_id IS NULL) OR (scope_kind = 'organization' AND scope_id IS NOT NULL));
CREATE UNIQUE INDEX idx_teams_platform_name ON teams(name) WHERE scope_kind = 'platform';
CREATE UNIQUE INDEX idx_teams_organization_name ON teams(scope_id, name) WHERE scope_kind = 'organization';
