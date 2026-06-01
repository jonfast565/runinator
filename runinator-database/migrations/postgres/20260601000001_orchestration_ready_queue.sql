ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS orchestration_version BIGINT NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS workflow_orchestration_events (
    event_id TEXT PRIMARY KEY,
    workflow_run_id BIGINT NOT NULL REFERENCES workflow_runs(id),
    workflow_node_run_id BIGINT NULL REFERENCES workflow_node_runs(id),
    node_id TEXT NULL,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_ready_nodes (
    id BIGSERIAL PRIMARY KEY,
    source_event_id TEXT NOT NULL REFERENCES workflow_orchestration_events(event_id),
    workflow_run_id BIGINT NOT NULL REFERENCES workflow_runs(id),
    node_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    ready_at BIGINT NOT NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    claimed_by TEXT NULL,
    claimed_until BIGINT NULL,
    completed_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    UNIQUE(source_event_id, workflow_run_id, node_id)
);

ALTER TABLE workflow_action_dispatches ADD COLUMN IF NOT EXISTS claimed_by TEXT NULL;
ALTER TABLE workflow_action_dispatches ADD COLUMN IF NOT EXISTS claimed_until BIGINT NULL;

CREATE INDEX IF NOT EXISTS idx_workflow_orchestration_events_run ON workflow_orchestration_events(workflow_run_id, created_at);
CREATE INDEX IF NOT EXISTS idx_workflow_ready_nodes_claim ON workflow_ready_nodes(status, completed_at, ready_at, claimed_until);
CREATE INDEX IF NOT EXISTS idx_workflow_action_dispatches_claim ON workflow_action_dispatches(published_at, claimed_until, updated_at);
