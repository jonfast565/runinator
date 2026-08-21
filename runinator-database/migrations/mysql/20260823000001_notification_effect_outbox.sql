-- External notification sends are provider effects with their own durable outbox.  They must not
-- share the removed workflow_action_dispatches table or synthesize a node-run identity.
ALTER TABLE notification_deliveries ADD COLUMN dedupe_key VARCHAR(255) NULL;
ALTER TABLE notification_deliveries ADD COLUMN command_json LONGTEXT NULL;
ALTER TABLE notification_deliveries ADD COLUMN published_at BIGINT NULL;
ALTER TABLE notification_deliveries ADD COLUMN claimed_by VARCHAR(255) NULL;
ALTER TABLE notification_deliveries ADD COLUMN claimed_until BIGINT NULL;

UPDATE notification_deliveries
SET status = 'failed',
    last_error = 'notification delivery predates the VM notification outbox',
    updated_at = UNIX_TIMESTAMP()
WHERE command_json IS NULL AND status IN ('pending', 'dispatched');

CREATE UNIQUE INDEX idx_notification_deliveries_dedupe ON notification_deliveries(dedupe_key);
CREATE INDEX idx_notification_deliveries_claim
    ON notification_deliveries(published_at, claimed_until, updated_at);
