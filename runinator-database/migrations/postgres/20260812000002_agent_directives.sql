CREATE TABLE agent_directives (
    directive_id UUID PRIMARY KEY,
    replica_id UUID NOT NULL REFERENCES replicas(replica_id) ON DELETE CASCADE,
    kind_json TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    issued_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    published_at BIGINT NULL,
    completed_at BIGINT NULL,
    payload_json TEXT NOT NULL DEFAULT 'null',
    message TEXT NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    claimed_at BIGINT NULL,
    claimed_by_runtime_id TEXT NULL
);
CREATE INDEX idx_agent_directives_due ON agent_directives(state, expires_at, claimed_at);
CREATE INDEX idx_agent_directives_replica ON agent_directives(replica_id, issued_at DESC);
