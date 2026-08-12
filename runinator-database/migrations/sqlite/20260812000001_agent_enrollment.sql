ALTER TABLE api_keys ADD COLUMN principal_kind TEXT NOT NULL DEFAULT 'user';
ALTER TABLE api_keys ADD COLUMN org_id BLOB NULL;
UPDATE api_keys SET principal_kind = 'service' WHERE is_service = 1;

CREATE TABLE agent_enrollment_tokens (
    token_id TEXT PRIMARY KEY,
    sealed_secret BLOB NOT NULL,
    org_id BLOB NULL,
    labels_json TEXT NOT NULL,
    service_url TEXT NOT NULL,
    spki_pin TEXT NULL,
    expires_at INTEGER NOT NULL,
    consumed_at INTEGER NULL,
    issued_by BLOB NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_agent_enrollment_tokens_expiry ON agent_enrollment_tokens(expires_at, consumed_at);
