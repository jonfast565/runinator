ALTER TABLE orchestration_adapter_revisions
ADD COLUMN authentication TEXT NOT NULL DEFAULT '{"kind":"secrets","secret_bindings":{}}';

CREATE TABLE IF NOT EXISTS orchestration_adapter_poll_dispatches (
    id BLOB PRIMARY KEY,
    adapter_id BLOB NOT NULL,
    adapter_revision INTEGER NOT NULL,
    profile_id BLOB NOT NULL,
    claim_owner TEXT NOT NULL,
    command TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(adapter_id) REFERENCES orchestration_adapters(id) ON DELETE CASCADE
);
CREATE INDEX idx_adapter_poll_dispatches_adapter ON orchestration_adapter_poll_dispatches(adapter_id, state);
