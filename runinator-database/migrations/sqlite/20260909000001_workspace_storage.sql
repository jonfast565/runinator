ALTER TABLE workspace_snapshots RENAME COLUMN archive_uri TO revision_id;
CREATE TABLE workspace_objects (
    workspace_id BLOB NOT NULL REFERENCES durable_workspaces(id),
    object_id VARCHAR(64) NOT NULL,
    pack_id VARCHAR(128) NOT NULL,
    location_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, object_id)
);
CREATE INDEX idx_workspace_objects_pack ON workspace_objects(workspace_id, pack_id);
CREATE TABLE workspace_receipts (
    id BLOB PRIMARY KEY,
    workspace_id BLOB NOT NULL REFERENCES durable_workspaces(id),
    checkout_id BLOB NOT NULL,
    fence INTEGER NOT NULL,
    receipt_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    consumed INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_workspace_receipts_checkout ON workspace_receipts(checkout_id, consumed);
CREATE TABLE workspace_downloads (
    id BLOB PRIMARY KEY,
    workspace_id BLOB NOT NULL REFERENCES durable_workspaces(id),
    version INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    download_json TEXT NOT NULL
);
CREATE INDEX idx_workspace_downloads_expiry ON workspace_downloads(workspace_id, version, expires_at);
CREATE TABLE workspace_gc_state (
    workspace_id BLOB PRIMARY KEY REFERENCES durable_workspaces(id),
    token BLOB NOT NULL,
    fence INTEGER NOT NULL DEFAULT 0,
    lease_until INTEGER NOT NULL DEFAULT 0,
    last_revision INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE workspace_gc_objects (
    workspace_id BLOB NOT NULL,
    fence INTEGER NOT NULL,
    object_id VARCHAR(64) NOT NULL,
    pack_id VARCHAR(128) NOT NULL,
    location_json TEXT NOT NULL,
    PRIMARY KEY (workspace_id, fence, object_id)
);
CREATE TABLE workspace_retired_packs (
    workspace_id BLOB NOT NULL,
    pack_id VARCHAR(128) NOT NULL,
    delete_after INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, pack_id)
);
CREATE TABLE workspace_readers (
    id BLOB PRIMARY KEY,
    workspace_id BLOB NOT NULL REFERENCES durable_workspaces(id),
    version INTEGER NOT NULL,
    lease_until INTEGER NOT NULL
);
CREATE INDEX idx_workspace_readers_lease ON workspace_readers(workspace_id, lease_until);

ALTER TABLE workspace_snapshots RENAME TO workspace_snapshots_legacy;
CREATE TABLE workspace_snapshots (
    workflow_run_id BLOB NULL,
    workspace_id BLOB NOT NULL REFERENCES durable_workspaces(id),
    version INTEGER NOT NULL,
    effect_id BLOB NULL,
    attempt INTEGER NULL,
    revision_id TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    deleted_at INTEGER NULL,
    PRIMARY KEY (workspace_id, version),
    UNIQUE (workspace_id, effect_id, attempt)
);
INSERT INTO workspace_snapshots SELECT * FROM workspace_snapshots_legacy;
DROP TABLE workspace_snapshots_legacy;
CREATE INDEX idx_workspace_snapshot_uri ON workspace_snapshots(revision_id);
CREATE INDEX idx_workspace_snapshot_run ON workspace_snapshots(workspace_id, workflow_run_id, version);

CREATE TABLE workspace_transfers (
    id BLOB PRIMARY KEY,
    workspace_id BLOB NOT NULL REFERENCES durable_workspaces(id),
    version INTEGER NOT NULL,
    importing INTEGER NOT NULL,
    state VARCHAR(20) NOT NULL,
    token BLOB NOT NULL,
    lease_until INTEGER NOT NULL DEFAULT 0,
    progress INTEGER NOT NULL DEFAULT 0,
    archive_uri TEXT NULL,
    error TEXT NULL,
    expires_at INTEGER NOT NULL,
    job_json TEXT NOT NULL
);
CREATE INDEX idx_workspace_transfers_pending ON workspace_transfers(state, lease_until);
CREATE INDEX idx_workspace_transfers_roots ON workspace_transfers(workspace_id, version, expires_at);

CREATE INDEX idx_workspace_receipts_workspace ON workspace_receipts(workspace_id);
