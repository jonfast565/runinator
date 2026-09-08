ALTER TABLE ingress_control_events ADD COLUMN adapter TEXT NULL;
ALTER TABLE ingress_control_events ADD COLUMN caller_org_id BLOB NULL;
ALTER TABLE ingress_control_events ADD COLUMN claim_token BLOB NULL;
ALTER TABLE ingress_control_events ADD COLUMN claimed_until INTEGER NULL;
ALTER TABLE orchestration_adapter_poll_dispatches ADD COLUMN dry_run INTEGER NOT NULL DEFAULT FALSE;
ALTER TABLE orchestration_adapter_poll_dispatches ADD COLUMN deadline_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE orchestration_adapter_poll_dispatches ADD COLUMN result TEXT NULL;
ALTER TABLE orchestration_adapter_poll_dispatches ADD COLUMN last_error TEXT NULL;
ALTER TABLE orchestration_adapter_poll_dispatches ADD COLUMN publication_token BLOB NULL;
ALTER TABLE orchestration_adapter_poll_dispatches ADD COLUMN publication_until INTEGER NULL;
CREATE TABLE adapter_deliveries (
 id BLOB PRIMARY KEY, adapter_id BLOB NOT NULL, delivery_key VARCHAR(544) NOT NULL,
 data TEXT NOT NULL, state VARCHAR(32) NOT NULL,
 received_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
 claim_token BLOB NULL, claimed_until INTEGER NULL,
 UNIQUE(adapter_id, delivery_key)
);
CREATE INDEX idx_adapter_deliveries_state ON adapter_deliveries(state, received_at);
CREATE TABLE adapter_inspection (
 adapter_id BLOB PRIMARY KEY, mode VARCHAR(32) NOT NULL
);
CREATE TABLE orchestration_debug_controls (
 pipeline_id BLOB PRIMARY KEY, paused INTEGER NOT NULL, steps INTEGER NOT NULL
);

ALTER TABLE broker_messages ADD COLUMN adapter_id BLOB NULL;
ALTER TABLE broker_messages ADD COLUMN poll_attempt_id BLOB NULL;
CREATE INDEX idx_broker_messages_adapter ON broker_messages(adapter_id, occurred_at);
