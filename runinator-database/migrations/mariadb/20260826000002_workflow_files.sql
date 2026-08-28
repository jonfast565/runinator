-- User-uploaded file metadata for VM run parameters. Bytes stay in the blob store; rows make
-- ownership, current library revisions, and run-bound authorization durable.
CREATE TABLE IF NOT EXISTS workflow_files (
    id BINARY(16) PRIMARY KEY,
    scope VARCHAR(16) NOT NULL,
    org_id BINARY(16) NULL,
    owner_id BINARY(16) NULL,
    workflow_run_id BINARY(16) NULL,
    path TEXT NOT NULL,
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    sha256 VARCHAR(64) NOT NULL,
    uri TEXT NOT NULL,
    revision BIGINT NOT NULL DEFAULT 1,
    is_current BOOLEAN NOT NULL DEFAULT TRUE,
    archived BOOLEAN NOT NULL DEFAULT FALSE,
    created_at BIGINT NOT NULL,
    CONSTRAINT fk_workflow_files_run FOREIGN KEY (workflow_run_id)
        REFERENCES workflow_runs(id) ON DELETE CASCADE
);
CREATE INDEX idx_workflow_files_library
    ON workflow_files(scope, org_id, is_current, archived);
CREATE INDEX idx_workflow_files_run
    ON workflow_files(workflow_run_id, id);
