ALTER TABLE orchestration_adapter_revisions ADD COLUMN transport TEXT NOT NULL DEFAULT 'webhook';

CREATE TABLE orchestration_adapter_polls (
    adapter_id UUID PRIMARY KEY REFERENCES orchestration_adapters(id) ON DELETE CASCADE,
    revision BIGINT NOT NULL, checkpoint TEXT NOT NULL DEFAULT 'null', next_poll_at BIGINT NOT NULL,
    claimed_by TEXT NULL, claimed_until BIGINT NULL, last_attempt_at BIGINT NULL,
    last_success_at BIGINT NULL, last_error TEXT NULL
);
CREATE INDEX idx_orchestration_adapter_polls_due ON orchestration_adapter_polls(next_poll_at, claimed_until);
