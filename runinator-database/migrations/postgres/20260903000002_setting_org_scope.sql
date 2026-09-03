ALTER TABLE settings ADD COLUMN org_id UUID NULL;
ALTER TABLE settings DROP CONSTRAINT settings_pkey;
CREATE UNIQUE INDEX IF NOT EXISTS idx_settings_id ON settings(id);
CREATE UNIQUE INDEX idx_settings_org_alias
    ON settings(org_id, kind, scope, name) WHERE org_id IS NOT NULL;
CREATE UNIQUE INDEX idx_settings_platform_alias
    ON settings(kind, scope, name) WHERE org_id IS NULL;

CREATE UNIQUE INDEX idx_execution_profiles_platform_name
    ON execution_profiles(name) WHERE org_id IS NULL;
