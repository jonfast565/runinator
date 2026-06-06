-- Initial schema for the Runinator web service (MySQL/MariaDB).
-- Text columns that are primary keys, unique, indexed, or foreign keys use VARCHAR so they can
-- carry an index (MySQL cannot index a bare TEXT without a prefix length).

CREATE TABLE IF NOT EXISTS runs (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    status VARCHAR(64) NOT NULL,
    parameters LONGTEXT NOT NULL,
    output_json LONGTEXT NULL,
    message TEXT NULL,
    `trigger` TEXT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    workflow_run_id BIGINT NULL,
    workflow_node_id TEXT NULL
);

CREATE TABLE IF NOT EXISTS run_chunks (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    run_id BIGINT NOT NULL REFERENCES runs(id),
    sequence BIGINT NOT NULL,
    stream TEXT NOT NULL,
    content LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS run_artifacts (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    run_id BIGINT NOT NULL REFERENCES runs(id),
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    uri TEXT NOT NULL,
    metadata LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflows (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    version BIGINT NOT NULL,
    enabled TINYINT(1) NOT NULL,
    input_schema LONGTEXT NOT NULL,
    definition LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_triggers (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    workflow_id BIGINT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    kind VARCHAR(64) NOT NULL,
    enabled TINYINT(1) NOT NULL,
    configuration LONGTEXT NOT NULL,
    next_execution BIGINT NULL,
    blackout_start BIGINT NULL,
    blackout_end BIGINT NULL,
    metadata LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_runs (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    workflow_id BIGINT NOT NULL REFERENCES workflows(id),
    workflow_snapshot LONGTEXT NULL,
    status VARCHAR(64) NOT NULL,
    active_node_id TEXT NULL,
    parameters LONGTEXT NOT NULL,
    state LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    message TEXT NULL,
    name VARCHAR(255) NULL,
    scheduler_claimed_by VARCHAR(255) NULL,
    scheduler_claimed_until BIGINT NULL
);

CREATE TABLE IF NOT EXISTS workflow_node_runs (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    workflow_run_id BIGINT NOT NULL REFERENCES workflow_runs(id),
    node_id TEXT NOT NULL,
    status VARCHAR(64) NOT NULL,
    attempt BIGINT NOT NULL DEFAULT 0,
    parameters LONGTEXT NOT NULL,
    output_json LONGTEXT NULL,
    state LONGTEXT NOT NULL,
    transition_reason TEXT NULL,
    created_at BIGINT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    message TEXT NULL
);

CREATE TABLE IF NOT EXISTS workflow_node_chunks (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    workflow_node_run_id BIGINT NOT NULL REFERENCES workflow_node_runs(id) ON DELETE CASCADE,
    sequence BIGINT NOT NULL,
    stream TEXT NOT NULL,
    content LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_node_artifacts (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    workflow_node_run_id BIGINT NOT NULL REFERENCES workflow_node_runs(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    uri TEXT NOT NULL,
    metadata LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_result_events (
    event_id VARCHAR(64) PRIMARY KEY,
    workflow_run_id BIGINT NOT NULL,
    workflow_node_run_id BIGINT NOT NULL,
    node_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_trigger_firings (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    trigger_id BIGINT NOT NULL REFERENCES workflow_triggers(id) ON DELETE CASCADE,
    fire_key VARCHAR(255) NOT NULL,
    workflow_run_id BIGINT NULL REFERENCES workflow_runs(id),
    scheduler_id TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE(trigger_id, fire_key)
);

CREATE TABLE IF NOT EXISTS catalog_items (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    uri VARCHAR(512) NOT NULL UNIQUE,
    item_type VARCHAR(128) NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    document LONGTEXT NOT NULL,
    metadata LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS automation_records (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    record_type VARCHAR(128) NOT NULL,
    workflow_run_id BIGINT NULL,
    external_item_id BIGINT NULL,
    node_id TEXT NULL,
    provider TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    external_id TEXT NOT NULL,
    status VARCHAR(64) NOT NULL,
    title TEXT NULL,
    url TEXT NULL,
    body TEXT NULL,
    path TEXT NULL,
    prompt TEXT NULL,
    approval_type TEXT NULL,
    resolved_by TEXT NULL,
    resolved_at BIGINT NULL,
    metadata LONGTEXT NOT NULL,
    data LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS idempotency_keys (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    scope VARCHAR(255) NOT NULL,
    `key` VARCHAR(255) NOT NULL,
    result LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE(scope, `key`)
);

CREATE TABLE IF NOT EXISTS workflow_action_dispatches (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    dedupe_key VARCHAR(255) NOT NULL UNIQUE,
    command_json LONGTEXT NOT NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    published_at BIGINT NULL,
    last_error TEXT NULL
);

CREATE TABLE IF NOT EXISTS notifications (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    workflow_run_id BIGINT NULL,
    workflow_node_id TEXT NULL,
    channel TEXT NOT NULL,
    severity TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NULL,
    target TEXT NULL,
    metadata LONGTEXT NOT NULL,
    read_at BIGINT NULL,
    created_at BIGINT NOT NULL
);

CREATE INDEX idx_notifications_unread ON notifications(read_at, created_at);
CREATE INDEX idx_runs_status ON runs(status);
CREATE INDEX idx_run_chunks_run_sequence ON run_chunks(run_id, sequence);
CREATE INDEX idx_workflows_name ON workflows(name);
CREATE INDEX idx_workflow_runs_status ON workflow_runs(status);
CREATE INDEX idx_workflow_runs_scheduler_claim ON workflow_runs(status, scheduler_claimed_until);
CREATE INDEX idx_workflow_triggers_workflow ON workflow_triggers(workflow_id);
CREATE INDEX idx_workflow_triggers_due ON workflow_triggers(enabled, kind, next_execution);
CREATE INDEX idx_workflow_node_runs_workflow_run ON workflow_node_runs(workflow_run_id);
CREATE INDEX idx_workflow_node_chunks_node_sequence ON workflow_node_chunks(workflow_node_run_id, sequence);
CREATE INDEX idx_workflow_node_artifacts_node ON workflow_node_artifacts(workflow_node_run_id);
CREATE INDEX idx_workflow_result_events_node ON workflow_result_events(workflow_node_run_id);
CREATE INDEX idx_workflow_trigger_firings_trigger ON workflow_trigger_firings(trigger_id);
CREATE INDEX idx_catalog_items_type ON catalog_items(item_type);
CREATE INDEX idx_automation_records_type ON automation_records(record_type);
CREATE INDEX idx_automation_records_workflow_run ON automation_records(workflow_run_id);
CREATE INDEX idx_automation_records_external_item ON automation_records(external_item_id);
CREATE INDEX idx_workflow_action_dispatches_pending ON workflow_action_dispatches(published_at, updated_at);
