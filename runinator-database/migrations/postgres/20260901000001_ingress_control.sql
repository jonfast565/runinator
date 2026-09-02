CREATE TABLE IF NOT EXISTS ingress_control_gates (
    target_kind TEXT NOT NULL,
    target_id UUID NOT NULL,
    owner_scope_kind TEXT NOT NULL,
    owner_scope_id UUID NULL,
    mode TEXT NOT NULL,
    updated_by UUID NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY(target_kind, target_id)
);

CREATE TABLE IF NOT EXISTS ingress_control_events (
    id UUID PRIMARY KEY,
    target_kind TEXT NOT NULL,
    target_id UUID NOT NULL,
    owner_scope_kind TEXT NOT NULL,
    owner_scope_id UUID NULL,
    gate_mode TEXT NOT NULL,
    source TEXT NOT NULL,
    event_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    correlation_key TEXT NOT NULL,
    payload TEXT NOT NULL,
    provenance TEXT NOT NULL DEFAULT '{}',
    occurred_at BIGINT NULL,
    state TEXT NOT NULL,
    reviewed_by UUID NULL,
    last_error TEXT NULL,
    received_at BIGINT NOT NULL,
    resolved_at BIGINT NULL,
    UNIQUE(target_kind, target_id, source, event_id)
);
CREATE INDEX IF NOT EXISTS idx_ingress_control_events_queue
    ON ingress_control_events(target_kind, target_id, state, received_at, id);
CREATE INDEX IF NOT EXISTS idx_ingress_control_events_scope
    ON ingress_control_events(owner_scope_kind, owner_scope_id, received_at);

CREATE TABLE IF NOT EXISTS broker_ingress_sessions (
    scope_key TEXT PRIMARY KEY,
    scope_kind TEXT NOT NULL,
    scope_id UUID NULL,
    mode TEXT NOT NULL,
    updated_by UUID NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS broker_ingress_messages (
    id UUID PRIMARY KEY,
    scope_key TEXT NOT NULL,
    scope_kind TEXT NOT NULL,
    scope_id UUID NULL,
    delivery_id UUID NOT NULL,
    dedupe_key TEXT NOT NULL,
    command_kind TEXT NOT NULL,
    command TEXT NOT NULL,
    state TEXT NOT NULL,
    reviewed_by UUID NULL,
    last_error TEXT NULL,
    received_at BIGINT NOT NULL,
    resolved_at BIGINT NULL,
    UNIQUE(scope_key, dedupe_key)
);
CREATE INDEX IF NOT EXISTS idx_broker_ingress_messages_queue
    ON broker_ingress_messages(scope_key, state, received_at, id);
CREATE INDEX IF NOT EXISTS idx_broker_ingress_messages_resolved
    ON broker_ingress_messages(resolved_at, id);
