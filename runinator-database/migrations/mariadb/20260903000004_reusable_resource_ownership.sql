RENAME TABLE resource_ownership TO resource_ownership_legacy;
CREATE TABLE resource_ownership (
    resource_type VARCHAR(64) NOT NULL,
    resource_id BINARY(16) NOT NULL,
    tenant_scope_kind VARCHAR(32) NOT NULL,
    tenant_scope_id BINARY(16) NULL,
    owner_scope_kind VARCHAR(32) NOT NULL,
    owner_scope_id BINARY(16) NULL,
    created_by BINARY(16) NULL,
    authz_version BIGINT NOT NULL DEFAULT 1,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
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

INSERT INTO resource_ownership SELECT 'setting', id,
    IF(org_id IS NULL, 'platform', 'organization'), org_id,
    IF(org_id IS NULL, 'platform', 'organization'), org_id,
    NULL, 1, updated_at, updated_at FROM settings;
INSERT INTO resource_ownership SELECT 'execution_profile', id,
    IF(org_id IS NULL, 'platform', 'organization'), org_id,
    IF(org_id IS NULL, 'platform', 'organization'), org_id,
    NULL, 1, created_at, updated_at FROM execution_profiles;
INSERT INTO resource_ownership SELECT 'orchestration_adapter', id, 'organization', org_id,
    'organization', org_id, NULL, 1, created_at, updated_at FROM orchestration_adapters;
INSERT INTO resource_ownership SELECT 'library_file', id, 'organization', org_id,
    IF(owner_id IS NULL, 'organization', 'user'), COALESCE(owner_id, org_id), owner_id,
    1, created_at, created_at FROM workflow_files WHERE scope = 'library' AND org_id IS NOT NULL;
INSERT INTO resource_ownership SELECT 'notification_policy', id,
    IF(org_id IS NULL, 'platform', 'organization'), org_id,
    IF(org_id IS NULL, 'platform', 'organization'), org_id,
    NULL, 1, created_at, updated_at FROM notification_policies WHERE workflow_id IS NULL;
