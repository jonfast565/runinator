-- Initial schema for the Runinator web service (Postgres).

CREATE TABLE IF NOT EXISTS runs (
    id BIGSERIAL PRIMARY KEY,
    status TEXT NOT NULL,
    parameters TEXT NOT NULL,
    output_json TEXT NULL,
    message TEXT NULL,
    trigger TEXT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    workflow_run_id BIGINT NULL,
    workflow_node_id TEXT NULL
);

CREATE TABLE IF NOT EXISTS run_chunks (
    id BIGSERIAL PRIMARY KEY,
    run_id BIGINT NOT NULL REFERENCES runs(id),
    sequence BIGINT NOT NULL,
    stream TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS run_artifacts (
    id BIGSERIAL PRIMARY KEY,
    run_id BIGINT NOT NULL REFERENCES runs(id),
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    uri TEXT NOT NULL,
    metadata TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflows (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    version BIGINT NOT NULL,
    enabled BOOLEAN NOT NULL,
    input_schema TEXT NOT NULL,
    definition TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_triggers (
    id BIGSERIAL PRIMARY KEY,
    workflow_id BIGINT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    enabled BOOLEAN NOT NULL,
    configuration TEXT NOT NULL DEFAULT '{}',
    next_execution BIGINT NULL,
    blackout_start BIGINT NULL,
    blackout_end BIGINT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_runs (
    id BIGSERIAL PRIMARY KEY,
    workflow_id BIGINT NOT NULL REFERENCES workflows(id),
    workflow_snapshot TEXT NULL,
    status TEXT NOT NULL,
    active_node_id TEXT NULL,
    parameters TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    message TEXT NULL,
    name TEXT NULL,
    scheduler_claimed_by TEXT NULL,
    scheduler_claimed_until BIGINT NULL
);

-- Forward-compat for older databases predating these columns.
ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS workflow_snapshot TEXT NULL;
ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS name TEXT NULL;
ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS scheduler_claimed_by TEXT NULL;
ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS scheduler_claimed_until BIGINT NULL;

CREATE TABLE IF NOT EXISTS workflow_node_runs (
    id BIGSERIAL PRIMARY KEY,
    workflow_run_id BIGINT NOT NULL REFERENCES workflow_runs(id),
    node_id TEXT NOT NULL,
    status TEXT NOT NULL,
    attempt BIGINT NOT NULL DEFAULT 0,
    parameters TEXT NOT NULL DEFAULT '{}',
    output_json TEXT NULL,
    state TEXT NOT NULL DEFAULT '{}',
    transition_reason TEXT NULL,
    created_at BIGINT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    message TEXT NULL
);

CREATE TABLE IF NOT EXISTS workflow_node_chunks (
    id BIGSERIAL PRIMARY KEY,
    workflow_node_run_id BIGINT NOT NULL REFERENCES workflow_node_runs(id) ON DELETE CASCADE,
    sequence BIGINT NOT NULL,
    stream TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_node_artifacts (
    id BIGSERIAL PRIMARY KEY,
    workflow_node_run_id BIGINT NOT NULL REFERENCES workflow_node_runs(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    uri TEXT NOT NULL,
    metadata TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_result_events (
    event_id TEXT PRIMARY KEY,
    workflow_run_id BIGINT NOT NULL,
    workflow_node_run_id BIGINT NOT NULL,
    node_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_trigger_firings (
    id BIGSERIAL PRIMARY KEY,
    trigger_id BIGINT NOT NULL REFERENCES workflow_triggers(id) ON DELETE CASCADE,
    fire_key TEXT NOT NULL,
    workflow_run_id BIGINT NULL REFERENCES workflow_runs(id),
    scheduler_id TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE(trigger_id, fire_key)
);

CREATE TABLE IF NOT EXISTS catalog_items (
    id BIGSERIAL PRIMARY KEY,
    uri TEXT NOT NULL UNIQUE,
    item_type TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    document TEXT NOT NULL DEFAULT '{}',
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS automation_records (
    id BIGSERIAL PRIMARY KEY,
    record_type TEXT NOT NULL,
    workflow_run_id BIGINT NULL,
    external_item_id BIGINT NULL,
    node_id TEXT NULL,
    provider TEXT NOT NULL DEFAULT '',
    resource_type TEXT NOT NULL DEFAULT '',
    external_id TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT '',
    title TEXT NULL,
    url TEXT NULL,
    body TEXT NULL,
    path TEXT NULL,
    prompt TEXT NULL,
    approval_type TEXT NULL,
    resolved_by TEXT NULL,
    resolved_at BIGINT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    data TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS idempotency_keys (
    id BIGSERIAL PRIMARY KEY,
    scope TEXT NOT NULL,
    key TEXT NOT NULL,
    result TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    UNIQUE(scope, key)
);

CREATE TABLE IF NOT EXISTS workflow_action_dispatches (
    id BIGSERIAL PRIMARY KEY,
    dedupe_key TEXT NOT NULL UNIQUE,
    command_json TEXT NOT NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    published_at BIGINT NULL,
    last_error TEXT NULL
);

CREATE TABLE IF NOT EXISTS notifications (
    id BIGSERIAL PRIMARY KEY,
    workflow_run_id BIGINT NULL,
    workflow_node_id TEXT NULL,
    channel TEXT NOT NULL,
    severity TEXT NOT NULL DEFAULT 'info',
    title TEXT NOT NULL,
    body TEXT NULL,
    target TEXT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    read_at BIGINT NULL,
    created_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_notifications_unread ON notifications(read_at, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_runs_status ON runs(status);
CREATE INDEX IF NOT EXISTS idx_run_chunks_run_sequence ON run_chunks(run_id, sequence);
CREATE INDEX IF NOT EXISTS idx_workflows_name ON workflows(name);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_status ON workflow_runs(status);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_scheduler_claim ON workflow_runs(status, scheduler_claimed_until);
CREATE INDEX IF NOT EXISTS idx_workflow_triggers_workflow ON workflow_triggers(workflow_id);
CREATE INDEX IF NOT EXISTS idx_workflow_triggers_due ON workflow_triggers(enabled, kind, next_execution);
CREATE INDEX IF NOT EXISTS idx_workflow_node_runs_workflow_run ON workflow_node_runs(workflow_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_node_chunks_node_sequence ON workflow_node_chunks(workflow_node_run_id, sequence);
CREATE INDEX IF NOT EXISTS idx_workflow_node_artifacts_node ON workflow_node_artifacts(workflow_node_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_result_events_node ON workflow_result_events(workflow_node_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_trigger_firings_trigger ON workflow_trigger_firings(trigger_id);
CREATE INDEX IF NOT EXISTS idx_catalog_items_type ON catalog_items(item_type);
CREATE INDEX IF NOT EXISTS idx_automation_records_type ON automation_records(record_type);
CREATE INDEX IF NOT EXISTS idx_automation_records_workflow_run ON automation_records(workflow_run_id);
CREATE INDEX IF NOT EXISTS idx_automation_records_external_item ON automation_records(external_item_id);
CREATE INDEX IF NOT EXISTS idx_workflow_action_dispatches_pending ON workflow_action_dispatches(published_at, updated_at);
