-- Team names are tenant-local. A team can be owned by the platform or one organization, never
-- by a user or another team.
ALTER TABLE teams DROP INDEX name;
ALTER TABLE teams
    ADD CONSTRAINT teams_scope_check
    CHECK ((scope_kind = 'platform' AND scope_id IS NULL) OR (scope_kind = 'organization' AND scope_id IS NOT NULL));
ALTER TABLE teams
    ADD COLUMN scope_key VARCHAR(64)
    AS (CASE WHEN scope_kind = 'platform' THEN 'platform' ELSE CONCAT('organization:', HEX(scope_id)) END) STORED;
CREATE UNIQUE INDEX idx_teams_scope_name ON teams(scope_key, name);
