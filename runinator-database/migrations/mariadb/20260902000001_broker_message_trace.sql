ALTER TABLE broker_ingress_sessions
    ADD COLUMN expires_at BIGINT NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS broker_messages (
    id BINARY(16) PRIMARY KEY,
    channel VARCHAR(32) NOT NULL,
    direction VARCHAR(16) NOT NULL,
    message_kind VARCHAR(64) NOT NULL,
    workflow_run_id BINARY(16) NULL,
    delivery_id BINARY(16) NULL,
    dedupe_key VARCHAR(512) NULL,
    trace_id BINARY(16) NULL,
    payload LONGTEXT NOT NULL,
    occurred_at BIGINT NOT NULL,
    KEY idx_broker_messages_workflow(workflow_run_id, occurred_at, id),
    KEY idx_broker_messages_channel(channel, occurred_at, id),
    KEY idx_broker_messages_retention(occurred_at, id)
);
