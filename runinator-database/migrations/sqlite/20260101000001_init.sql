-- Initial schema for the Runinator web service (SQLite).
-- All surrogate keys (primary/foreign/event ids) are UUIDs generated app-side and stored as
-- BLOB (sqlx encodes uuid::Uuid as a 16-byte blob for sqlite). Columns that hold workflow graph
-- node identifiers or external identity strings stay TEXT.

CREATE TABLE IF NOT EXISTS runs (
    id BLOB PRIMARY KEY,
    status TEXT NOT NULL,
    parameters TEXT NOT NULL,
    output_json TEXT NULL,
    message TEXT NULL,
    trigger TEXT NOT NULL,
    started_at INTEGER NULL,
    finished_at INTEGER NULL,
    created_at INTEGER NOT NULL,
    workflow_run_id BLOB NULL,
    workflow_node_id TEXT NULL
);

CREATE TABLE IF NOT EXISTS run_chunks (
    id BLOB PRIMARY KEY,
    run_id BLOB NOT NULL REFERENCES runs(id),
    sequence INTEGER NOT NULL,
    stream TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS run_artifacts (
    id BLOB PRIMARY KEY,
    run_id BLOB NOT NULL REFERENCES runs(id),
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    uri TEXT NOT NULL,
    metadata TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workflows (
    id BLOB PRIMARY KEY,
    name TEXT NOT NULL,
    version INTEGER NOT NULL,
    enabled BOOL NOT NULL,
    input_schema TEXT NOT NULL,
    definition TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_triggers (
    id BLOB PRIMARY KEY,
    workflow_id BLOB NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    enabled BOOL NOT NULL,
    configuration TEXT NOT NULL DEFAULT '{}',
    next_execution INTEGER NULL,
    blackout_start INTEGER NULL,
    blackout_end INTEGER NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_runs (
    id BLOB PRIMARY KEY,
    workflow_id BLOB NOT NULL REFERENCES workflows(id),
    workflow_snapshot TEXT NULL,
    status TEXT NOT NULL,
    active_node_id TEXT NULL,
    parameters TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    started_at INTEGER NULL,
    finished_at INTEGER NULL,
    message TEXT NULL,
    name TEXT NULL,
    scheduler_claimed_by TEXT NULL,
    scheduler_claimed_until INTEGER NULL
);

CREATE TABLE IF NOT EXISTS workflow_node_runs (
    id BLOB PRIMARY KEY,
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id),
    node_id TEXT NOT NULL,
    status TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0,
    parameters TEXT NOT NULL DEFAULT '{}',
    output_json TEXT NULL,
    state TEXT NOT NULL DEFAULT '{}',
    transition_reason TEXT NULL,
    created_at INTEGER NOT NULL,
    started_at INTEGER NULL,
    finished_at INTEGER NULL,
    message TEXT NULL
);

CREATE TABLE IF NOT EXISTS workflow_node_chunks (
    id BLOB PRIMARY KEY,
    workflow_node_run_id BLOB NOT NULL REFERENCES workflow_node_runs(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    stream TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_node_artifacts (
    id BLOB PRIMARY KEY,
    workflow_node_run_id BLOB NOT NULL REFERENCES workflow_node_runs(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    uri TEXT NOT NULL,
    metadata TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_result_events (
    event_id BLOB PRIMARY KEY,
    workflow_run_id BLOB NOT NULL,
    workflow_node_run_id BLOB NOT NULL,
    node_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_trigger_firings (
    id BLOB PRIMARY KEY,
    trigger_id BLOB NOT NULL REFERENCES workflow_triggers(id) ON DELETE CASCADE,
    fire_key TEXT NOT NULL,
    workflow_run_id BLOB NULL REFERENCES workflow_runs(id),
    scheduler_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(trigger_id, fire_key)
);

CREATE TABLE IF NOT EXISTS catalog_items (
    id BLOB PRIMARY KEY,
    uri TEXT NOT NULL UNIQUE,
    item_type TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    document TEXT NOT NULL DEFAULT '{}',
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS automation_records (
    id BLOB PRIMARY KEY,
    record_type TEXT NOT NULL,
    workflow_run_id BLOB NULL,
    external_item_id BLOB NULL,
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
    resolved_at INTEGER NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    data TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS idempotency_keys (
    id BLOB PRIMARY KEY,
    scope TEXT NOT NULL,
    key TEXT NOT NULL,
    result TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    UNIQUE(scope, key)
);

CREATE TABLE IF NOT EXISTS workflow_action_dispatches (
    id BLOB PRIMARY KEY,
    dedupe_key TEXT NOT NULL UNIQUE,
    command_json TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    published_at INTEGER NULL,
    last_error TEXT NULL
);

CREATE TABLE IF NOT EXISTS notifications (
    id BLOB PRIMARY KEY,
    workflow_run_id BLOB NULL,
    workflow_node_id TEXT NULL,
    channel TEXT NOT NULL,
    severity TEXT NOT NULL DEFAULT 'info',
    title TEXT NOT NULL,
    body TEXT NULL,
    target TEXT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    read_at INTEGER NULL,
    created_at INTEGER NOT NULL
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
