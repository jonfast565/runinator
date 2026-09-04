CREATE TABLE IF NOT EXISTS execution_profile_agent_statuses (
    profile_id BLOB NOT NULL REFERENCES execution_profiles(id) ON DELETE CASCADE,
    agent_id BLOB NOT NULL,
    config_digest TEXT NOT NULL,
    approval TEXT NOT NULL,
    last_seen_at INTEGER NOT NULL,
    last_attempt_at INTEGER NULL,
    last_success_at INTEGER NULL,
    last_error TEXT NULL,
    PRIMARY KEY (profile_id, agent_id, config_digest)
);

CREATE INDEX IF NOT EXISTS idx_execution_profile_agent_statuses_current
    ON execution_profile_agent_statuses(profile_id, config_digest, last_seen_at DESC);

CREATE TABLE IF NOT EXISTS execution_profile_operations (
    id BLOB PRIMARY KEY NOT NULL,
    profile_id BLOB NOT NULL REFERENCES execution_profiles(id) ON DELETE CASCADE,
    config_digest TEXT NOT NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    active_key INTEGER NULL,
    requested_at INTEGER NOT NULL,
    requested_by BLOB NULL,
    claimed_by BLOB NULL,
    started_at INTEGER NULL,
    lease_expires_at INTEGER NULL,
    completed_at INTEGER NULL,
    error TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_execution_profile_operations_latest
    ON execution_profile_operations(profile_id, config_digest, requested_at DESC);

CREATE INDEX IF NOT EXISTS idx_execution_profile_operations_pending
    ON execution_profile_operations(state, requested_at ASC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_execution_profile_operations_one_active
    ON execution_profile_operations(profile_id, config_digest, active_key);

UPDATE execution_profiles
SET health = CASE
        WHEN enabled = 0 THEN 'disabled'
        WHEN current_revision IS NULL THEN 'unpublished'
        ELSE 'ready'
    END,
    last_error = NULL
WHERE health IN ('testing', 'error');
