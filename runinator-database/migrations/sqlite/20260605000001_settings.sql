-- unified config/secret store, moved off the per-pod credential file so values survive pod
-- restarts and are shared across web-service replicas. `value` holds ciphertext (the web service
-- encrypts before writing); `updated_at` is unix seconds, used for import reconciliation.
CREATE TABLE IF NOT EXISTS settings (
    kind TEXT NOT NULL,
    scope TEXT NOT NULL,
    name TEXT NOT NULL,
    value BLOB NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (kind, scope, name)
);
