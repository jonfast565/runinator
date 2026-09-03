-- A global resource belongs to the platform. Individual and team authority over it is expressed
-- through a grant, never by changing the resource's tenant or owner scope.
INSERT INTO resource_grants (id, resource_type, resource_id, principal_type, principal_id, permission, created_at)
SELECT randomblob(16), resource_type, resource_id, owner_scope_kind, owner_scope_id, 'own', updated_at
FROM resource_ownership ownership
WHERE tenant_scope_kind = 'platform'
  AND owner_scope_kind IN ('user', 'team')
ON CONFLICT (resource_type, resource_id, principal_type, principal_id)
DO UPDATE SET permission = excluded.permission, created_at = excluded.created_at;
UPDATE resource_ownership
SET owner_scope_kind = 'platform', owner_scope_id = NULL, authz_version = authz_version + 1
WHERE tenant_scope_kind = 'platform' AND owner_scope_kind <> 'platform';

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
    CHECK (tenant_scope_kind IN ('platform', 'organization')),
    CHECK (owner_scope_kind IN ('platform', 'organization', 'team', 'user')),
    CHECK ((tenant_scope_kind = 'platform' AND tenant_scope_id IS NULL) OR (tenant_scope_kind = 'organization' AND tenant_scope_id IS NOT NULL)),
    CHECK ((owner_scope_kind = 'platform' AND owner_scope_id IS NULL) OR (owner_scope_kind <> 'platform' AND owner_scope_id IS NOT NULL)),
    CHECK (tenant_scope_kind <> 'platform' OR owner_scope_kind = 'platform')
);
INSERT INTO resource_ownership SELECT * FROM resource_ownership_legacy;
DROP TABLE resource_ownership_legacy;
CREATE INDEX idx_resource_ownership_owner ON resource_ownership(owner_scope_kind, owner_scope_id);
CREATE INDEX idx_resource_ownership_tenant ON resource_ownership(tenant_scope_kind, tenant_scope_id);
