CREATE TABLE calendar_subscriptions (
    id BLOB PRIMARY KEY,
    principal_id BLOB NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope_kind TEXT NOT NULL,
    scope_id BLOB NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_calendar_subscriptions_principal ON calendar_subscriptions(principal_id);
