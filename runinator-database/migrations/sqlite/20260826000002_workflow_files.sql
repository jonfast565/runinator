-- User-uploaded file metadata for VM run parameters. Bytes stay in the blob store; rows make
-- ownership, current library revisions, and run-bound authorization durable.
CREATE TABLE IF NOT EXISTS workflow_files (
    id BLOB PRIMARY KEY,
    scope TEXT NOT NULL,
    org_id BLOB NULL,
    owner_id BLOB NULL,
    workflow_run_id BLOB NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    uri TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    is_current INTEGER NOT NULL DEFAULT 1,
    archived INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_workflow_files_library
    ON workflow_files(scope, org_id, path, is_current, archived);
CREATE INDEX IF NOT EXISTS idx_workflow_files_run
    ON workflow_files(workflow_run_id, id);
