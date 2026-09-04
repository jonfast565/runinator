CREATE TABLE durable_workspaces (
    id BLOB PRIMARY KEY,
    org_id BLOB NULL,
    updated_at INTEGER NOT NULL,
    tenant_key TEXT NOT NULL,
    workspace_key TEXT NOT NULL,
    head_version INTEGER NOT NULL DEFAULT 0,
    revision INTEGER NOT NULL DEFAULT 1,
    deleted_at INTEGER NULL,
    metadata_json TEXT NOT NULL,
    UNIQUE (tenant_key, workspace_key)
);
CREATE TABLE workspace_snapshots (
    workflow_run_id BLOB NOT NULL,
    workspace_id BLOB NOT NULL REFERENCES durable_workspaces(id),
    version INTEGER NOT NULL,
    effect_id BLOB NOT NULL,
    attempt INTEGER NOT NULL,
    archive_uri TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    deleted_at INTEGER NULL,
    PRIMARY KEY (workspace_id, version),
    UNIQUE (workspace_id, effect_id, attempt)
);
CREATE TABLE workspace_checkouts (
    id BLOB PRIMARY KEY,
    workspace_id BLOB NOT NULL REFERENCES durable_workspaces(id),
    effect_id BLOB NOT NULL,
    attempt INTEGER NOT NULL,
    base_version INTEGER NOT NULL,
    writer INTEGER NOT NULL,
    fence INTEGER NOT NULL,
    leased_until INTEGER NOT NULL,
    checkout_json TEXT NOT NULL,
    UNIQUE (workspace_id, effect_id, attempt)
);
CREATE INDEX idx_workspace_checkouts_lease ON workspace_checkouts(workspace_id, leased_until);

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
    CHECK (resource_type IN ('workflow', 'pipeline', 'function_package', 'console_session', 'setting', 'execution_profile', 'orchestration_adapter', 'library_file', 'notification_policy', 'workspace')),
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

CREATE TABLE workspace_pins (
    workspace_id BLOB NOT NULL,
    version INTEGER NOT NULL,
    workflow_run_id BLOB NOT NULL,
    PRIMARY KEY (workspace_id, version, workflow_run_id)
);

CREATE INDEX idx_workspace_snapshot_uri ON workspace_snapshots(archive_uri);

CREATE INDEX idx_workspace_snapshot_run ON workspace_snapshots(workspace_id, workflow_run_id, version);
