ALTER TABLE workspace_snapshots RENAME COLUMN archive_uri TO revision_id;
CREATE TABLE workspace_objects (
    workspace_id BINARY(16) NOT NULL REFERENCES durable_workspaces(id),
    object_id VARCHAR(64) NOT NULL,
    pack_id VARCHAR(128) NOT NULL,
    location_json LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (workspace_id, object_id)
);
CREATE INDEX idx_workspace_objects_pack ON workspace_objects(workspace_id, pack_id);
CREATE TABLE workspace_receipts (
    id BINARY(16) PRIMARY KEY,
    workspace_id BINARY(16) NOT NULL REFERENCES durable_workspaces(id),
    checkout_id BINARY(16) NOT NULL,
    fence BIGINT NOT NULL,
    receipt_json LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    consumed BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX idx_workspace_receipts_checkout ON workspace_receipts(checkout_id, consumed);
CREATE TABLE workspace_downloads (
    id BINARY(16) PRIMARY KEY,
    workspace_id BINARY(16) NOT NULL REFERENCES durable_workspaces(id),
    version BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    download_json LONGTEXT NOT NULL
);
CREATE INDEX idx_workspace_downloads_expiry ON workspace_downloads(workspace_id, version, expires_at);
CREATE TABLE workspace_gc_state (
    workspace_id BINARY(16) PRIMARY KEY REFERENCES durable_workspaces(id),
    token BINARY(16) NOT NULL,
    fence BIGINT NOT NULL DEFAULT 0,
    lease_until BIGINT NOT NULL DEFAULT 0,
    last_revision BIGINT NOT NULL DEFAULT 0
);
CREATE TABLE workspace_gc_objects (
    workspace_id BINARY(16) NOT NULL,
    fence BIGINT NOT NULL,
    object_id VARCHAR(64) NOT NULL,
    pack_id VARCHAR(128) NOT NULL,
    location_json LONGTEXT NOT NULL,
    PRIMARY KEY (workspace_id, fence, object_id)
);
CREATE TABLE workspace_retired_packs (
    workspace_id BINARY(16) NOT NULL,
    pack_id VARCHAR(128) NOT NULL,
    delete_after BIGINT NOT NULL,
    PRIMARY KEY (workspace_id, pack_id)
);
CREATE TABLE workspace_readers (
    id BINARY(16) PRIMARY KEY,
    workspace_id BINARY(16) NOT NULL REFERENCES durable_workspaces(id),
    version BIGINT NOT NULL,
    lease_until BIGINT NOT NULL
);
CREATE INDEX idx_workspace_readers_lease ON workspace_readers(workspace_id, lease_until);

ALTER TABLE workspace_snapshots MODIFY workflow_run_id BINARY(16) NULL, MODIFY effect_id BINARY(16) NULL, MODIFY attempt BIGINT NULL;

CREATE TABLE workspace_transfers (
    id BINARY(16) PRIMARY KEY,
    workspace_id BINARY(16) NOT NULL REFERENCES durable_workspaces(id),
    version BIGINT NOT NULL,
    importing BIGINT NOT NULL,
    state VARCHAR(20) NOT NULL,
    token BINARY(16) NOT NULL,
    lease_until BIGINT NOT NULL DEFAULT 0,
    progress BIGINT NOT NULL DEFAULT 0,
    archive_uri LONGTEXT NULL,
    error LONGTEXT NULL,
    expires_at BIGINT NOT NULL,
    job_json LONGTEXT NOT NULL
);
CREATE INDEX idx_workspace_transfers_pending ON workspace_transfers(state, lease_until);
CREATE INDEX idx_workspace_transfers_roots ON workspace_transfers(workspace_id, version, expires_at);

CREATE INDEX idx_workspace_receipts_workspace ON workspace_receipts(workspace_id);
