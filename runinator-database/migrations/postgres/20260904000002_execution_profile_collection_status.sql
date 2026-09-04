CREATE TABLE IF NOT EXISTS execution_profile_agent_statuses (
    profile_id UUID NOT NULL REFERENCES execution_profiles(id) ON DELETE CASCADE,
    agent_id UUID NOT NULL,
    config_digest TEXT NOT NULL,
    approval TEXT NOT NULL,
    last_seen_at BIGINT NOT NULL,
    last_attempt_at BIGINT NULL,
    last_success_at BIGINT NULL,
    last_error TEXT NULL,
    PRIMARY KEY (profile_id, agent_id, config_digest)
);

CREATE INDEX IF NOT EXISTS idx_execution_profile_agent_statuses_current
    ON execution_profile_agent_statuses(profile_id, config_digest, last_seen_at DESC);

CREATE TABLE IF NOT EXISTS execution_profile_operations (
    id UUID PRIMARY KEY NOT NULL,
    profile_id UUID NOT NULL REFERENCES execution_profiles(id) ON DELETE CASCADE,
    config_digest TEXT NOT NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    active_key BIGINT NULL,
    requested_at BIGINT NOT NULL,
    requested_by UUID NULL,
    claimed_by UUID NULL,
    started_at BIGINT NULL,
    lease_expires_at BIGINT NULL,
    completed_at BIGINT NULL,
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
        WHEN enabled = FALSE THEN 'disabled'
        WHEN current_revision IS NULL THEN 'unpublished'
        ELSE 'ready'
    END,
    last_error = NULL
WHERE health IN ('testing', 'error');
