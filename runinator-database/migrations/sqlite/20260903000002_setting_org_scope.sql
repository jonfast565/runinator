-- Existing settings become platform-owned. Rebuild the table because the original primary key
-- omitted organization ownership and would prevent two organizations using the same alias.
CREATE TABLE settings_scoped (
    id BLOB NOT NULL,
    org_id BLOB NULL,
    kind TEXT NOT NULL,
    scope TEXT NOT NULL,
    name TEXT NOT NULL,
    value BLOB NOT NULL,
    updated_at INTEGER NOT NULL
);

INSERT INTO settings_scoped (id, org_id, kind, scope, name, value, updated_at)
SELECT id, NULL, kind, scope, name, value, updated_at FROM settings;

DROP TABLE settings;
ALTER TABLE settings_scoped RENAME TO settings;

CREATE UNIQUE INDEX idx_settings_id ON settings(id);
CREATE UNIQUE INDEX idx_settings_org_alias
    ON settings(org_id, kind, scope, name) WHERE org_id IS NOT NULL;
CREATE UNIQUE INDEX idx_settings_platform_alias
    ON settings(kind, scope, name) WHERE org_id IS NULL;

CREATE UNIQUE INDEX idx_execution_profiles_platform_name
    ON execution_profiles(name) WHERE org_id IS NULL;
