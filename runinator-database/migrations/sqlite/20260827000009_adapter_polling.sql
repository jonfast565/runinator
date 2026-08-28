ALTER TABLE orchestration_adapter_revisions ADD COLUMN transport TEXT NOT NULL DEFAULT 'webhook';

CREATE TABLE IF NOT EXISTS orchestration_adapter_polls (
    adapter_id BLOB PRIMARY KEY,
    revision INTEGER NOT NULL,
    checkpoint TEXT NOT NULL DEFAULT 'null',
    next_poll_at INTEGER NOT NULL,
    claimed_by TEXT NULL,
    claimed_until INTEGER NULL,
    last_attempt_at INTEGER NULL,
    last_success_at INTEGER NULL,
    last_error TEXT NULL,
    FOREIGN KEY(adapter_id) REFERENCES orchestration_adapters(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_orchestration_adapter_polls_due ON orchestration_adapter_polls(next_poll_at, claimed_until);
