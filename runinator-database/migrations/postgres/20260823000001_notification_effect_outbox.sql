-- External notification sends are provider effects with their own durable outbox.  They must not
-- share the removed workflow_action_dispatches table or synthesize a node-run identity.
ALTER TABLE notification_deliveries ADD COLUMN IF NOT EXISTS dedupe_key TEXT NULL;
ALTER TABLE notification_deliveries ADD COLUMN IF NOT EXISTS command_json TEXT NULL;
ALTER TABLE notification_deliveries ADD COLUMN IF NOT EXISTS published_at BIGINT NULL;
ALTER TABLE notification_deliveries ADD COLUMN IF NOT EXISTS claimed_by TEXT NULL;
ALTER TABLE notification_deliveries ADD COLUMN IF NOT EXISTS claimed_until BIGINT NULL;

UPDATE notification_deliveries
SET status = 'failed',
    last_error = 'notification delivery predates the VM notification outbox',
    updated_at = EXTRACT(EPOCH FROM NOW())::BIGINT
WHERE command_json IS NULL AND status IN ('pending', 'dispatched');

CREATE UNIQUE INDEX IF NOT EXISTS idx_notification_deliveries_dedupe
    ON notification_deliveries(dedupe_key) WHERE dedupe_key IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_notification_deliveries_claim
    ON notification_deliveries(published_at, claimed_until, updated_at);
