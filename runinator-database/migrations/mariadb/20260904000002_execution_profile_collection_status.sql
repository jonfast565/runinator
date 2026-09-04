CREATE TABLE IF NOT EXISTS execution_profile_agent_statuses (
    profile_id BINARY(16) NOT NULL,
    agent_id BINARY(16) NOT NULL,
    config_digest VARCHAR(64) NOT NULL,
    approval VARCHAR(32) NOT NULL,
    last_seen_at BIGINT NOT NULL,
    last_attempt_at BIGINT NULL,
    last_success_at BIGINT NULL,
    last_error TEXT NULL,
    PRIMARY KEY (profile_id, agent_id, config_digest),
    CONSTRAINT fk_execution_profile_agent_status_profile FOREIGN KEY (profile_id)
        REFERENCES execution_profiles(id) ON DELETE CASCADE
);

CREATE INDEX idx_execution_profile_agent_statuses_current
    ON execution_profile_agent_statuses(profile_id, config_digest, last_seen_at);

CREATE TABLE IF NOT EXISTS execution_profile_operations (
    id BINARY(16) PRIMARY KEY NOT NULL,
    profile_id BINARY(16) NOT NULL,
    config_digest VARCHAR(64) NOT NULL,
    kind VARCHAR(32) NOT NULL,
    state VARCHAR(32) NOT NULL,
    active_key BIGINT NULL,
    requested_at BIGINT NOT NULL,
    requested_by BINARY(16) NULL,
    claimed_by BINARY(16) NULL,
    started_at BIGINT NULL,
    lease_expires_at BIGINT NULL,
    completed_at BIGINT NULL,
    error TEXT NULL,
    CONSTRAINT fk_execution_profile_operation_profile FOREIGN KEY (profile_id)
        REFERENCES execution_profiles(id) ON DELETE CASCADE
);

CREATE INDEX idx_execution_profile_operations_latest
    ON execution_profile_operations(profile_id, config_digest, requested_at);

CREATE INDEX idx_execution_profile_operations_pending
    ON execution_profile_operations(state, requested_at);

CREATE UNIQUE INDEX idx_execution_profile_operations_one_active
    ON execution_profile_operations(profile_id, config_digest, active_key);

UPDATE execution_profiles
SET health = CASE
        WHEN enabled = FALSE THEN 'disabled'
        WHEN current_revision IS NULL THEN 'unpublished'
        ELSE 'ready'
    END,
    last_error = NULL
WHERE health IN ('testing', 'error');
