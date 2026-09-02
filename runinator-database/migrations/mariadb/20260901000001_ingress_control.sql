CREATE TABLE IF NOT EXISTS ingress_control_gates (
    target_kind VARCHAR(16) NOT NULL,
    target_id BINARY(16) NOT NULL,
    owner_scope_kind VARCHAR(32) NOT NULL,
    owner_scope_id BINARY(16) NULL,
    mode VARCHAR(32) NOT NULL,
    updated_by BINARY(16) NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY(target_kind, target_id)
);

CREATE TABLE IF NOT EXISTS ingress_control_events (
    id BINARY(16) PRIMARY KEY,
    target_kind VARCHAR(16) NOT NULL,
    target_id BINARY(16) NOT NULL,
    owner_scope_kind VARCHAR(32) NOT NULL,
    owner_scope_id BINARY(16) NULL,
    gate_mode VARCHAR(32) NOT NULL,
    source VARCHAR(128) NOT NULL,
    event_id VARCHAR(512) NOT NULL,
    event_type VARCHAR(256) NOT NULL,
    correlation_key VARCHAR(255) NOT NULL,
    payload LONGTEXT NOT NULL,
    provenance LONGTEXT NOT NULL,
    occurred_at BIGINT NULL,
    state VARCHAR(32) NOT NULL,
    reviewed_by BINARY(16) NULL,
    last_error TEXT NULL,
    received_at BIGINT NOT NULL,
    resolved_at BIGINT NULL,
    UNIQUE KEY idx_ingress_control_event_dedupe(target_kind, target_id, source, event_id),
    KEY idx_ingress_control_events_queue(target_kind, target_id, state, received_at, id),
    KEY idx_ingress_control_events_scope(owner_scope_kind, owner_scope_id, received_at)
);

CREATE TABLE IF NOT EXISTS broker_ingress_sessions (
    scope_key VARCHAR(64) PRIMARY KEY,
    scope_kind VARCHAR(32) NOT NULL,
    scope_id BINARY(16) NULL,
    mode VARCHAR(64) NOT NULL,
    updated_by BINARY(16) NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS broker_ingress_messages (
    id BINARY(16) PRIMARY KEY,
    scope_key VARCHAR(64) NOT NULL,
    scope_kind VARCHAR(32) NOT NULL,
    scope_id BINARY(16) NULL,
    delivery_id BINARY(16) NOT NULL,
    dedupe_key VARCHAR(512) NOT NULL,
    command_kind VARCHAR(64) NOT NULL,
    command LONGTEXT NOT NULL,
    state VARCHAR(32) NOT NULL,
    reviewed_by BINARY(16) NULL,
    last_error TEXT NULL,
    received_at BIGINT NOT NULL,
    resolved_at BIGINT NULL,
    UNIQUE KEY idx_broker_ingress_message_dedupe(scope_key, dedupe_key),
    KEY idx_broker_ingress_messages_queue(scope_key, state, received_at, id),
    KEY idx_broker_ingress_messages_resolved(resolved_at, id)
);
