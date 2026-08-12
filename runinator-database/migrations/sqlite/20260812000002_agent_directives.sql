CREATE TABLE agent_directives (
    directive_id BLOB PRIMARY KEY,
    replica_id BLOB NOT NULL REFERENCES replicas(replica_id) ON DELETE CASCADE,
    kind_json TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    issued_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    published_at INTEGER NULL,
    completed_at INTEGER NULL,
    payload_json TEXT NOT NULL DEFAULT 'null',
    message TEXT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    claimed_at INTEGER NULL,
    claimed_by_runtime_id TEXT NULL
);
CREATE INDEX idx_agent_directives_due ON agent_directives(state, expires_at, claimed_at);
CREATE INDEX idx_agent_directives_replica ON agent_directives(replica_id, issued_at DESC);
