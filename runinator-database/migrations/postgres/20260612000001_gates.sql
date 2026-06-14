CREATE TABLE IF NOT EXISTS gates (
    id UUID PRIMARY KEY,
    workflow_run_id UUID NOT NULL,
    node_id TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL DEFAULT 'manual',
    status TEXT NOT NULL DEFAULT 'pending',
    label TEXT NULL,
    reason TEXT NULL,
    resolved_by TEXT NULL,
    resolved_at BIGINT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    data TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_gates_workflow_run ON gates(workflow_run_id);
CREATE INDEX IF NOT EXISTS idx_gates_status ON gates(status);
