ALTER TABLE settings
    ADD COLUMN org_id BINARY(16) NULL,
    DROP PRIMARY KEY,
    ADD COLUMN platform_owner TINYINT
        AS (CASE WHEN org_id IS NULL THEN 1 ELSE NULL END) STORED,
    ADD UNIQUE INDEX idx_settings_org_alias (org_id, kind, scope, name),
    ADD UNIQUE INDEX idx_settings_platform_alias (platform_owner, kind, scope, name);

ALTER TABLE execution_profiles
    ADD COLUMN platform_owner TINYINT
        AS (CASE WHEN org_id IS NULL THEN 1 ELSE NULL END) STORED,
    ADD UNIQUE INDEX idx_execution_profiles_platform_name (platform_owner, name);
