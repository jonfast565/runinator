CREATE TABLE IF NOT EXISTS notification_policies (
    id UUID PRIMARY KEY,
    workflow_id UUID NULL REFERENCES workflows(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    event TEXT NOT NULL,
    severity TEXT NOT NULL DEFAULT 'warning',
    channel TEXT NOT NULL DEFAULT 'in_app',
    target TEXT NULL,
    threshold_seconds BIGINT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    managed_by TEXT NULL,
    configuration TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_notification_policies_lookup
    ON notification_policies(enabled, event, workflow_id);

CREATE TABLE IF NOT EXISTS notification_deliveries (
    id UUID PRIMARY KEY,
    notification_id UUID NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
    policy_id UUID NULL,
    channel TEXT NOT NULL,
    target TEXT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts BIGINT NOT NULL DEFAULT 0,
    last_error TEXT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_notification_deliveries_status
    ON notification_deliveries(status, created_at);

-- engine-emitted notifications carry a dedupe key so a policy that keeps matching on every scan
-- tick collapses onto one row. left null by manual posts, and postgres treats nulls as distinct.
ALTER TABLE notifications ADD COLUMN IF NOT EXISTS dedupe_key TEXT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_notifications_dedupe_key
    ON notifications(dedupe_key) WHERE dedupe_key IS NOT NULL;
