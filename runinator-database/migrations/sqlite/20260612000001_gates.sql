CREATE TABLE IF NOT EXISTS gates (
    id BLOB PRIMARY KEY,
    workflow_run_id BLOB NOT NULL,
    node_id TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL DEFAULT 'manual',
    status TEXT NOT NULL DEFAULT 'pending',
    label TEXT NULL,
    reason TEXT NULL,
    resolved_by TEXT NULL,
    resolved_at INTEGER NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    data TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_gates_workflow_run ON gates(workflow_run_id);
CREATE INDEX IF NOT EXISTS idx_gates_status ON gates(status);
