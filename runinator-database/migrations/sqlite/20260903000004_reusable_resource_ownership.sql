ALTER TABLE resource_ownership RENAME TO resource_ownership_legacy;
CREATE TABLE resource_ownership (
    resource_type TEXT NOT NULL,
    resource_id BLOB NOT NULL,
    tenant_scope_kind TEXT NOT NULL,
    tenant_scope_id BLOB NULL,
    owner_scope_kind TEXT NOT NULL,
    owner_scope_id BLOB NULL,
    created_by BLOB NULL,
    authz_version INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (resource_type, resource_id),
    CHECK (resource_type IN ('workflow', 'pipeline', 'function_package', 'console_session', 'setting', 'execution_profile', 'orchestration_adapter', 'library_file', 'notification_policy')),
    CHECK (tenant_scope_kind IN ('platform', 'organization', 'team', 'user')),
    CHECK (owner_scope_kind IN ('platform', 'organization', 'team', 'user')),
    CHECK ((tenant_scope_kind = 'platform' AND tenant_scope_id IS NULL) OR (tenant_scope_kind <> 'platform' AND tenant_scope_id IS NOT NULL)),
    CHECK ((owner_scope_kind = 'platform' AND owner_scope_id IS NULL) OR (owner_scope_kind <> 'platform' AND owner_scope_id IS NOT NULL))
);
INSERT INTO resource_ownership SELECT * FROM resource_ownership_legacy;
DROP TABLE resource_ownership_legacy;
CREATE INDEX idx_resource_ownership_owner ON resource_ownership(owner_scope_kind, owner_scope_id);
CREATE INDEX idx_resource_ownership_tenant ON resource_ownership(tenant_scope_kind, tenant_scope_id);

INSERT INTO resource_ownership
SELECT 'setting', id,
       CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
       CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
       NULL, 1, updated_at, updated_at FROM settings;
INSERT INTO resource_ownership
SELECT 'execution_profile', id,
       CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
       CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
       NULL, 1, created_at, updated_at FROM execution_profiles;
INSERT INTO resource_ownership
SELECT 'orchestration_adapter', id, 'organization', org_id, 'organization', org_id,
       NULL, 1, created_at, updated_at FROM orchestration_adapters;
INSERT INTO resource_ownership
SELECT 'library_file', id, 'organization', org_id,
       CASE WHEN owner_id IS NULL THEN 'organization' ELSE 'user' END,
       COALESCE(owner_id, org_id), owner_id, 1, created_at, created_at
FROM workflow_files WHERE scope = 'library' AND org_id IS NOT NULL;
INSERT INTO resource_ownership
SELECT 'notification_policy', id,
       CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
       CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
       NULL, 1, created_at, updated_at
FROM notification_policies WHERE workflow_id IS NULL;
