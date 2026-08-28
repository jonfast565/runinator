-- the parenthesized default is not a style choice. mysql 8 rejects a literal default on a TEXT
-- column outright ("BLOB, TEXT, GEOMETRY or JSON column can't have a default value"), which stops
-- this migration and every one after it; mariadb accepts the literal form, so the break is
-- invisible against the engine this workspace runs by default. both engines accept the expression
-- form, so it is what keeps the two in step.
ALTER TABLE orchestration_adapter_revisions ADD COLUMN transport TEXT NOT NULL DEFAULT ('webhook');

CREATE TABLE orchestration_adapter_polls (
    adapter_id BINARY(16) PRIMARY KEY,
    revision BIGINT NOT NULL, checkpoint TEXT NOT NULL, next_poll_at BIGINT NOT NULL,
    claimed_by TEXT NULL, claimed_until BIGINT NULL, last_attempt_at BIGINT NULL,
    last_success_at BIGINT NULL, last_error TEXT NULL,
    CONSTRAINT fk_orchestration_adapter_poll FOREIGN KEY (adapter_id) REFERENCES orchestration_adapters(id) ON DELETE CASCADE
);
CREATE INDEX idx_orchestration_adapter_polls_due ON orchestration_adapter_polls(next_poll_at, claimed_until);
