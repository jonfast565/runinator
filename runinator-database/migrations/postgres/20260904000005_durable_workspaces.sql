CREATE TABLE durable_workspaces (
    id UUID PRIMARY KEY,
    org_id UUID NULL,
    updated_at BIGINT NOT NULL,
    tenant_key TEXT NOT NULL,
    workspace_key TEXT NOT NULL,
    head_version BIGINT NOT NULL DEFAULT 0,
    revision BIGINT NOT NULL DEFAULT 1,
    deleted_at BIGINT NULL,
    metadata_json TEXT NOT NULL,
    UNIQUE (tenant_key, workspace_key)
);
CREATE TABLE workspace_snapshots (
    workflow_run_id UUID NOT NULL,
    workspace_id UUID NOT NULL REFERENCES durable_workspaces(id),
    version BIGINT NOT NULL,
    effect_id UUID NOT NULL,
    attempt BIGINT NOT NULL,
    archive_uri TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    deleted_at BIGINT NULL,
    PRIMARY KEY (workspace_id, version),
    UNIQUE (workspace_id, effect_id, attempt)
);
CREATE TABLE workspace_checkouts (
    id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES durable_workspaces(id),
    effect_id UUID NOT NULL,
    attempt BIGINT NOT NULL,
    base_version BIGINT NOT NULL,
    writer BIGINT NOT NULL,
    fence BIGINT NOT NULL,
    leased_until BIGINT NOT NULL,
    checkout_json TEXT NOT NULL,
    UNIQUE (workspace_id, effect_id, attempt)
);
CREATE INDEX idx_workspace_checkouts_lease ON workspace_checkouts(workspace_id, leased_until);

ALTER TABLE resource_ownership DROP CONSTRAINT resource_ownership_resource_type_check;
ALTER TABLE resource_ownership ADD CONSTRAINT resource_ownership_resource_type_check
CHECK (resource_type IN ('workflow', 'pipeline', 'function_package', 'console_session', 'setting', 'execution_profile', 'orchestration_adapter', 'library_file', 'notification_policy', 'workspace'));

CREATE TABLE workspace_pins (
    workspace_id UUID NOT NULL,
    version BIGINT NOT NULL,
    workflow_run_id UUID NOT NULL,
    PRIMARY KEY (workspace_id, version, workflow_run_id)
);

CREATE INDEX idx_workspace_snapshot_uri ON workspace_snapshots(archive_uri);

CREATE INDEX idx_workspace_snapshot_run ON workspace_snapshots(workspace_id, workflow_run_id, version);
