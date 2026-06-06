ALTER TABLE workflow_runs ADD COLUMN orchestration_version BIGINT NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS workflow_orchestration_events (
    event_id VARCHAR(64) PRIMARY KEY,
    workflow_run_id BIGINT NOT NULL REFERENCES workflow_runs(id),
    workflow_node_run_id BIGINT NULL REFERENCES workflow_node_runs(id),
    node_id TEXT NULL,
    event_type TEXT NOT NULL,
    payload LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_ready_nodes (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    source_event_id VARCHAR(64) NOT NULL REFERENCES workflow_orchestration_events(event_id),
    workflow_run_id BIGINT NOT NULL REFERENCES workflow_runs(id),
    node_id VARCHAR(255) NOT NULL,
    status VARCHAR(64) NOT NULL DEFAULT 'queued',
    ready_at BIGINT NOT NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    claimed_by VARCHAR(255) NULL,
    claimed_until BIGINT NULL,
    completed_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    UNIQUE(source_event_id, workflow_run_id, node_id)
);

ALTER TABLE workflow_action_dispatches ADD COLUMN claimed_by VARCHAR(255) NULL;
ALTER TABLE workflow_action_dispatches ADD COLUMN claimed_until BIGINT NULL;

CREATE INDEX idx_workflow_orchestration_events_run ON workflow_orchestration_events(workflow_run_id, created_at);
CREATE INDEX idx_workflow_ready_nodes_claim ON workflow_ready_nodes(status, completed_at, ready_at, claimed_until);
CREATE INDEX idx_workflow_action_dispatches_claim ON workflow_action_dispatches(published_at, claimed_until, updated_at);
