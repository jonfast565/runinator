CREATE TABLE durable_workspaces (
    id BINARY(16) PRIMARY KEY,
    org_id BINARY(16) NULL,
    updated_at BIGINT NOT NULL,
    tenant_key VARCHAR(36) NOT NULL,
    workspace_key VARCHAR(255) NOT NULL,
    head_version BIGINT NOT NULL DEFAULT 0,
    revision BIGINT NOT NULL DEFAULT 1,
    deleted_at BIGINT NULL,
    metadata_json LONGTEXT NOT NULL,
    UNIQUE (tenant_key, workspace_key)
);
CREATE TABLE workspace_snapshots (
    workflow_run_id BINARY(16) NOT NULL,
    workspace_id BINARY(16) NOT NULL REFERENCES durable_workspaces(id),
    version BIGINT NOT NULL,
    effect_id BINARY(16) NOT NULL,
    attempt BIGINT NOT NULL,
    archive_uri VARCHAR(512) NOT NULL,
    snapshot_json LONGTEXT NOT NULL,
    deleted_at BIGINT NULL,
    PRIMARY KEY (workspace_id, version),
    UNIQUE (workspace_id, effect_id, attempt)
);
CREATE TABLE workspace_checkouts (
    id BINARY(16) PRIMARY KEY,
    workspace_id BINARY(16) NOT NULL REFERENCES durable_workspaces(id),
    effect_id BINARY(16) NOT NULL,
    attempt BIGINT NOT NULL,
    base_version BIGINT NOT NULL,
    writer BIGINT NOT NULL,
    fence BIGINT NOT NULL,
    leased_until BIGINT NOT NULL,
    checkout_json LONGTEXT NOT NULL,
    UNIQUE (workspace_id, effect_id, attempt)
);
CREATE INDEX idx_workspace_checkouts_lease ON workspace_checkouts(workspace_id, leased_until);

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
    CHECK (resource_type IN ('workflow', 'pipeline', 'function_package', 'console_session', 'setting', 'execution_profile', 'orchestration_adapter', 'library_file', 'notification_policy', 'workspace')),
    CHECK (tenant_scope_kind IN ('platform', 'organization')),
    CHECK (tenant_scope_kind <> 'platform' OR owner_scope_kind = 'platform'),
    CHECK (owner_scope_kind IN ('platform', 'organization', 'team', 'user')),
    CHECK ((tenant_scope_kind = 'platform' AND tenant_scope_id IS NULL) OR (tenant_scope_kind <> 'platform' AND tenant_scope_id IS NOT NULL)),
    CHECK ((owner_scope_kind = 'platform' AND owner_scope_id IS NULL) OR (owner_scope_kind <> 'platform' AND owner_scope_id IS NOT NULL))
);
INSERT INTO resource_ownership SELECT * FROM resource_ownership_legacy;
DROP TABLE resource_ownership_legacy;
CREATE INDEX idx_resource_ownership_owner ON resource_ownership(owner_scope_kind, owner_scope_id);
CREATE INDEX idx_resource_ownership_tenant ON resource_ownership(tenant_scope_kind, tenant_scope_id);


CREATE TABLE workspace_pins (
    workspace_id BINARY(16) NOT NULL,
    version BIGINT NOT NULL,
    workflow_run_id BINARY(16) NOT NULL,
    PRIMARY KEY (workspace_id, version, workflow_run_id)
);

CREATE INDEX idx_workspace_snapshot_uri ON workspace_snapshots(archive_uri);

CREATE INDEX idx_workspace_snapshot_run ON workspace_snapshots(workspace_id, workflow_run_id, version);
