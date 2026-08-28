CREATE TABLE agent_directives (
    directive_id BINARY(16) PRIMARY KEY,
    replica_id BINARY(16) NOT NULL,
    kind_json TEXT NOT NULL,
    state VARCHAR(16) NOT NULL DEFAULT 'pending',
    issued_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    published_at BIGINT NULL,
    completed_at BIGINT NULL,
    payload_json TEXT NOT NULL,
    message TEXT NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    claimed_at BIGINT NULL,
    claimed_by_runtime_id VARCHAR(255) NULL,
    CONSTRAINT fk_agent_directives_replica FOREIGN KEY (replica_id) REFERENCES replicas(replica_id) ON DELETE CASCADE,
    INDEX idx_agent_directives_due (state, expires_at, claimed_at),
    INDEX idx_agent_directives_replica (replica_id, issued_at)
);
