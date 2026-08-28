ALTER TABLE api_keys ADD COLUMN principal_kind VARCHAR(16) NOT NULL DEFAULT 'user';
ALTER TABLE api_keys ADD COLUMN org_id BINARY(16) NULL;
UPDATE api_keys SET principal_kind = 'service' WHERE is_service = 1;

CREATE TABLE agent_enrollment_tokens (
    token_id VARCHAR(64) PRIMARY KEY,
    sealed_secret LONGBLOB NOT NULL,
    org_id BINARY(16) NULL,
    labels_json TEXT NOT NULL,
    service_url TEXT NOT NULL,
    spki_pin TEXT NULL,
    expires_at BIGINT NOT NULL,
    consumed_at BIGINT NULL,
    issued_by BINARY(16) NULL,
    created_at BIGINT NOT NULL,
    INDEX idx_agent_enrollment_tokens_expiry (expires_at, consumed_at)
);
