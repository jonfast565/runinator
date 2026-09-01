CREATE TABLE calendar_subscriptions (
    id UUID PRIMARY KEY,
    principal_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope_kind TEXT NOT NULL,
    scope_id UUID NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_at BIGINT NOT NULL
);

CREATE INDEX idx_calendar_subscriptions_principal ON calendar_subscriptions(principal_id);
