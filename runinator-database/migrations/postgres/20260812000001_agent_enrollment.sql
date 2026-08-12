ALTER TABLE api_keys ADD COLUMN principal_kind TEXT NOT NULL DEFAULT 'user';
ALTER TABLE api_keys ADD COLUMN org_id UUID NULL;
UPDATE api_keys SET principal_kind = 'service' WHERE is_service = TRUE;

CREATE TABLE agent_enrollment_tokens (
    token_id TEXT PRIMARY KEY,
    sealed_secret BYTEA NOT NULL,
    org_id UUID NULL,
    labels_json TEXT NOT NULL,
    service_url TEXT NOT NULL,
    spki_pin TEXT NULL,
    expires_at BIGINT NOT NULL,
    consumed_at BIGINT NULL,
    issued_by UUID NULL,
    created_at BIGINT NOT NULL
);
CREATE INDEX idx_agent_enrollment_tokens_expiry ON agent_enrollment_tokens(expires_at, consumed_at);
