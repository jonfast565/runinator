CREATE TABLE IF NOT EXISTS notification_policies (
    id BINARY(16) PRIMARY KEY,
    workflow_id BINARY(16) NULL,
    name TEXT NOT NULL,
    event VARCHAR(64) NOT NULL,
    severity VARCHAR(32) NOT NULL DEFAULT 'warning',
    channel VARCHAR(32) NOT NULL DEFAULT 'in_app',
    target TEXT NULL,
    threshold_seconds BIGINT NULL,
    enabled TINYINT(1) NOT NULL DEFAULT 1,
    managed_by VARCHAR(64) NULL,
    configuration LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE INDEX idx_notification_policies_lookup
    ON notification_policies(enabled, event, workflow_id);

CREATE TABLE IF NOT EXISTS notification_deliveries (
    id BINARY(16) PRIMARY KEY,
    notification_id BINARY(16) NOT NULL,
    policy_id BINARY(16) NULL,
    channel VARCHAR(32) NOT NULL,
    target TEXT NULL,
    status VARCHAR(32) NOT NULL DEFAULT 'pending',
    attempts BIGINT NOT NULL DEFAULT 0,
    last_error TEXT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE INDEX idx_notification_deliveries_status
    ON notification_deliveries(status, created_at);

-- engine-emitted notifications carry a dedupe key so a policy that keeps matching on every scan
-- tick collapses onto one row. varchar rather than text so it can be indexed without a prefix
-- length; mysql unique indexes permit repeated nulls, which is what manual posts rely on.
ALTER TABLE notifications ADD COLUMN dedupe_key VARCHAR(255) NULL;

CREATE UNIQUE INDEX idx_notifications_dedupe_key ON notifications(dedupe_key);
