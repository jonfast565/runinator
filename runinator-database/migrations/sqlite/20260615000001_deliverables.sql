CREATE TABLE IF NOT EXISTS workflow_run_deliverables (
    id BLOB PRIMARY KEY,
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    artifact_id BLOB NOT NULL REFERENCES workflow_node_artifacts(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    uri TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_workflow_run_deliverables_run ON workflow_run_deliverables(workflow_run_id);
