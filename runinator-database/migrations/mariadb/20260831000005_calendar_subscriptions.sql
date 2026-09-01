CREATE TABLE calendar_subscriptions (
    id BINARY(16) PRIMARY KEY,
    principal_id BINARY(16) NOT NULL,
    scope_kind VARCHAR(32) NOT NULL,
    scope_id BINARY(16) NULL,
    token_hash VARCHAR(64) NOT NULL UNIQUE,
    created_at BIGINT NOT NULL,
    CONSTRAINT fk_calendar_subscriptions_principal FOREIGN KEY (principal_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_calendar_subscriptions_principal ON calendar_subscriptions(principal_id);
