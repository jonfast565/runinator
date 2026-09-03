ALTER TABLE notification_policies ADD COLUMN org_id BINARY(16) NULL;
ALTER TABLE notifications ADD COLUMN org_id BINARY(16) NULL;
ALTER TABLE notifications ADD COLUMN source_resource_type VARCHAR(64) NULL;
ALTER TABLE notifications ADD COLUMN source_resource_id BINARY(16) NULL;

UPDATE notifications SET
    org_id = (SELECT w.org_id FROM workflow_runs r JOIN workflows w ON w.id = r.workflow_id WHERE r.id = notifications.workflow_run_id),
    source_resource_type = 'workflow',
    source_resource_id = (SELECT r.workflow_id FROM workflow_runs r WHERE r.id = notifications.workflow_run_id)
WHERE workflow_run_id IS NOT NULL;
UPDATE notification_policies SET org_id =
    (SELECT w.org_id FROM workflows w WHERE w.id = notification_policies.workflow_id)
WHERE workflow_id IS NOT NULL;

DROP INDEX idx_notifications_dedupe_key ON notifications;
CREATE UNIQUE INDEX idx_notifications_dedupe_key ON notifications(dedupe_key);
CREATE INDEX idx_notifications_org_created ON notifications(org_id, created_at);
CREATE INDEX idx_notification_policies_org ON notification_policies(org_id, workflow_id);

CREATE TABLE notification_receipts (
    notification_id BINARY(16) NOT NULL,
    user_id BINARY(16) NOT NULL,
    read_at BIGINT NULL,
    dismissed_at BIGINT NULL,
    PRIMARY KEY (notification_id, user_id),
    CONSTRAINT fk_notification_receipts_notification
        FOREIGN KEY (notification_id) REFERENCES notifications(id) ON DELETE CASCADE
);
CREATE INDEX idx_notification_receipts_user
    ON notification_receipts(user_id, dismissed_at, read_at);
