ALTER TABLE orchestration_adapter_revisions
ADD COLUMN authentication TEXT NOT NULL DEFAULT '{"kind":"secrets","secret_bindings":{}}';

CREATE TABLE orchestration_adapter_poll_dispatches (
    id BINARY(16) PRIMARY KEY,
    adapter_id BINARY(16) NOT NULL,
    adapter_revision BIGINT NOT NULL,
    profile_id BINARY(16) NOT NULL,
    claim_owner TEXT NOT NULL,
    command LONGTEXT NOT NULL,
    state VARCHAR(32) NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_adapter_poll_dispatch_adapter FOREIGN KEY(adapter_id) REFERENCES orchestration_adapters(id) ON DELETE CASCADE
);
CREATE INDEX idx_adapter_poll_dispatches_adapter ON orchestration_adapter_poll_dispatches(adapter_id, state);
