ALTER TABLE workflow_runs ADD COLUMN orchestration_version INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS workflow_orchestration_events (
    event_id BLOB PRIMARY KEY,
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id),
    workflow_node_run_id BLOB NULL REFERENCES workflow_node_runs(id),
    node_id TEXT NULL,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_ready_nodes (
    id BLOB PRIMARY KEY,
    source_event_id BLOB NOT NULL REFERENCES workflow_orchestration_events(event_id),
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id),
    node_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    ready_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    claimed_by TEXT NULL,
    claimed_until INTEGER NULL,
    completed_at INTEGER NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(source_event_id, workflow_run_id, node_id)
);

ALTER TABLE workflow_action_dispatches ADD COLUMN claimed_by TEXT NULL;
ALTER TABLE workflow_action_dispatches ADD COLUMN claimed_until INTEGER NULL;

CREATE INDEX IF NOT EXISTS idx_workflow_orchestration_events_run ON workflow_orchestration_events(workflow_run_id, created_at);
CREATE INDEX IF NOT EXISTS idx_workflow_ready_nodes_claim ON workflow_ready_nodes(status, completed_at, ready_at, claimed_until);
CREATE INDEX IF NOT EXISTS idx_workflow_action_dispatches_claim ON workflow_action_dispatches(published_at, claimed_until, updated_at);
