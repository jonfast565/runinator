CREATE TABLE IF NOT EXISTS gates (
    id BINARY(16) PRIMARY KEY,
    workflow_run_id BINARY(16) NOT NULL,
    node_id TEXT NOT NULL,
    kind VARCHAR(32) NOT NULL,
    status VARCHAR(64) NOT NULL,
    label TEXT NULL,
    reason TEXT NULL,
    resolved_by TEXT NULL,
    resolved_at BIGINT NULL,
    metadata LONGTEXT NOT NULL,
    data LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE INDEX idx_gates_workflow_run ON gates(workflow_run_id);
CREATE INDEX idx_gates_status ON gates(status);
