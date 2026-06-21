-- durable record of dead-lettered broker messages and an authn/authz audit trail.
CREATE TABLE IF NOT EXISTS dead_letters (
    id UUID PRIMARY KEY,
    channel TEXT NOT NULL,
    event_id UUID NULL,
    dedupe_key TEXT NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    error TEXT NOT NULL DEFAULT '',
    payload TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_dead_letters_channel ON dead_letters(channel);
CREATE INDEX IF NOT EXISTS idx_dead_letters_created ON dead_letters(created_at);

CREATE TABLE IF NOT EXISTS audit_log (
    id UUID PRIMARY KEY,
    actor_id UUID NULL,
    actor_kind TEXT NOT NULL DEFAULT 'unknown',
    action TEXT NOT NULL,
    resource_type TEXT NULL,
    resource_id UUID NULL,
    outcome TEXT NOT NULL DEFAULT 'success',
    detail TEXT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_audit_log_actor ON audit_log(actor_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_action ON audit_log(action);
CREATE INDEX IF NOT EXISTS idx_audit_log_created ON audit_log(created_at);
