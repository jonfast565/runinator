-- durable record of dead-lettered broker messages and an authn/authz audit trail.
CREATE TABLE IF NOT EXISTS dead_letters (
    id BINARY(16) PRIMARY KEY,
    channel VARCHAR(64) NOT NULL,
    event_id BINARY(16) NULL,
    dedupe_key VARCHAR(255) NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    error TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at BIGINT NOT NULL
);
CREATE INDEX idx_dead_letters_channel ON dead_letters(channel);
CREATE INDEX idx_dead_letters_created ON dead_letters(created_at);

CREATE TABLE IF NOT EXISTS audit_log (
    id BINARY(16) PRIMARY KEY,
    actor_id BINARY(16) NULL,
    actor_kind VARCHAR(32) NOT NULL DEFAULT 'unknown',
    action VARCHAR(128) NOT NULL,
    resource_type VARCHAR(64) NULL,
    resource_id BINARY(16) NULL,
    outcome VARCHAR(32) NOT NULL DEFAULT 'success',
    detail TEXT NULL,
    metadata TEXT NOT NULL,
    created_at BIGINT NOT NULL
);
CREATE INDEX idx_audit_log_actor ON audit_log(actor_id);
CREATE INDEX idx_audit_log_action ON audit_log(action);
CREATE INDEX idx_audit_log_created ON audit_log(created_at);
