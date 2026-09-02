ALTER TABLE broker_ingress_sessions
    ADD COLUMN expires_at INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS broker_messages (
    id BLOB PRIMARY KEY,
    channel TEXT NOT NULL,
    direction TEXT NOT NULL,
    message_kind TEXT NOT NULL,
    workflow_run_id BLOB NULL,
    delivery_id BLOB NULL,
    dedupe_key TEXT NULL,
    trace_id BLOB NULL,
    payload TEXT NOT NULL,
    occurred_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_broker_messages_workflow
    ON broker_messages(workflow_run_id, occurred_at, id);
CREATE INDEX IF NOT EXISTS idx_broker_messages_channel
    ON broker_messages(channel, occurred_at, id);
CREATE INDEX IF NOT EXISTS idx_broker_messages_retention
    ON broker_messages(occurred_at, id);
