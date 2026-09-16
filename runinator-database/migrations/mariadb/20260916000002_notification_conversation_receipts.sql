ALTER TABLE notification_deliveries ADD COLUMN workflow_run_id CHAR(36) NULL;
ALTER TABLE notification_deliveries ADD COLUMN response_json LONGTEXT NULL;

CREATE INDEX idx_notification_deliveries_workflow_run
    ON notification_deliveries(workflow_run_id);
