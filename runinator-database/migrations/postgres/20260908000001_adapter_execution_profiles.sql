ALTER TABLE orchestration_adapter_revisions
ADD COLUMN authentication TEXT NOT NULL DEFAULT '{"kind":"secrets","secret_bindings":{}}';

CREATE TABLE orchestration_adapter_poll_dispatches (
    id UUID PRIMARY KEY,
    adapter_id UUID NOT NULL REFERENCES orchestration_adapters(id) ON DELETE CASCADE,
    adapter_revision BIGINT NOT NULL,
    profile_id UUID NOT NULL,
    claim_owner TEXT NOT NULL,
    command TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX idx_adapter_poll_dispatches_adapter ON orchestration_adapter_poll_dispatches(adapter_id, state);
