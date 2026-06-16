CREATE TABLE IF NOT EXISTS workflow_run_deliverables (
    id BINARY(16) PRIMARY KEY,
    workflow_run_id BINARY(16) NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    artifact_id BINARY(16) NOT NULL REFERENCES workflow_node_artifacts(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    uri TEXT NOT NULL,
    metadata LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE INDEX idx_workflow_run_deliverables_run ON workflow_run_deliverables(workflow_run_id);
