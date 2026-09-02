CREATE TABLE IF NOT EXISTS ingress_control_gates (
    target_kind TEXT NOT NULL,
    target_id BLOB NOT NULL,
    owner_scope_kind TEXT NOT NULL,
    owner_scope_id BLOB NULL,
    mode TEXT NOT NULL,
    updated_by BLOB NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(target_kind, target_id)
);

CREATE TABLE IF NOT EXISTS ingress_control_events (
    id BLOB PRIMARY KEY,
    target_kind TEXT NOT NULL,
    target_id BLOB NOT NULL,
    owner_scope_kind TEXT NOT NULL,
    owner_scope_id BLOB NULL,
    gate_mode TEXT NOT NULL,
    source TEXT NOT NULL,
    event_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    correlation_key TEXT NOT NULL,
    payload TEXT NOT NULL,
    provenance TEXT NOT NULL DEFAULT '{}',
    occurred_at INTEGER NULL,
    state TEXT NOT NULL,
    reviewed_by BLOB NULL,
    last_error TEXT NULL,
    received_at INTEGER NOT NULL,
    resolved_at INTEGER NULL,
    UNIQUE(target_kind, target_id, source, event_id)
);
CREATE INDEX IF NOT EXISTS idx_ingress_control_events_queue
    ON ingress_control_events(target_kind, target_id, state, received_at, id);
CREATE INDEX IF NOT EXISTS idx_ingress_control_events_scope
    ON ingress_control_events(owner_scope_kind, owner_scope_id, received_at);

CREATE TABLE IF NOT EXISTS broker_ingress_sessions (
    scope_key TEXT PRIMARY KEY,
    scope_kind TEXT NOT NULL,
    scope_id BLOB NULL,
    mode TEXT NOT NULL,
    updated_by BLOB NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS broker_ingress_messages (
    id BLOB PRIMARY KEY,
    scope_key TEXT NOT NULL,
    scope_kind TEXT NOT NULL,
    scope_id BLOB NULL,
    delivery_id BLOB NOT NULL,
    dedupe_key TEXT NOT NULL,
    command_kind TEXT NOT NULL,
    command TEXT NOT NULL,
    state TEXT NOT NULL,
    reviewed_by BLOB NULL,
    last_error TEXT NULL,
    received_at INTEGER NOT NULL,
    resolved_at INTEGER NULL,
    UNIQUE(scope_key, dedupe_key)
);
CREATE INDEX IF NOT EXISTS idx_broker_ingress_messages_queue
    ON broker_ingress_messages(scope_key, state, received_at, id);
CREATE INDEX IF NOT EXISTS idx_broker_ingress_messages_resolved
    ON broker_ingress_messages(resolved_at, id);
