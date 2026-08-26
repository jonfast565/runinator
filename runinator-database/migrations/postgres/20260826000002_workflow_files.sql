-- User-uploaded file metadata for VM run parameters. Bytes stay in the blob store; rows make
-- ownership, current library revisions, and run-bound authorization durable.
CREATE TABLE IF NOT EXISTS workflow_files (
    id UUID PRIMARY KEY,
    scope TEXT NOT NULL,
    org_id UUID NULL,
    owner_id UUID NULL,
    workflow_run_id UUID NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    sha256 TEXT NOT NULL,
    uri TEXT NOT NULL,
    revision BIGINT NOT NULL DEFAULT 1,
    is_current BOOLEAN NOT NULL DEFAULT TRUE,
    archived BOOLEAN NOT NULL DEFAULT FALSE,
    created_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_workflow_files_library
    ON workflow_files(scope, org_id, path, is_current, archived);
CREATE INDEX IF NOT EXISTS idx_workflow_files_run
    ON workflow_files(workflow_run_id, id);
