ALTER TABLE notification_deliveries ADD COLUMN workflow_run_id UUID NULL;
ALTER TABLE notification_deliveries ADD COLUMN response_json TEXT NULL;

CREATE INDEX IF NOT EXISTS idx_notification_deliveries_workflow_run
    ON notification_deliveries(workflow_run_id);
