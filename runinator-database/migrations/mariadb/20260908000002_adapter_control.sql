ALTER TABLE ingress_control_events ADD COLUMN adapter LONGTEXT NULL;
ALTER TABLE ingress_control_events ADD COLUMN caller_org_id BINARY(16) NULL;
ALTER TABLE ingress_control_events ADD COLUMN claim_token BINARY(16) NULL;
ALTER TABLE ingress_control_events ADD COLUMN claimed_until BIGINT NULL;
ALTER TABLE orchestration_adapter_poll_dispatches ADD COLUMN dry_run BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE orchestration_adapter_poll_dispatches ADD COLUMN deadline_at BIGINT NOT NULL DEFAULT 0;
ALTER TABLE orchestration_adapter_poll_dispatches ADD COLUMN result LONGTEXT NULL;
ALTER TABLE orchestration_adapter_poll_dispatches ADD COLUMN last_error LONGTEXT NULL;
ALTER TABLE orchestration_adapter_poll_dispatches ADD COLUMN publication_token BINARY(16) NULL;
ALTER TABLE orchestration_adapter_poll_dispatches ADD COLUMN publication_until BIGINT NULL;
CREATE TABLE adapter_deliveries (
 id BINARY(16) PRIMARY KEY, adapter_id BINARY(16) NOT NULL, delivery_key VARCHAR(544) NOT NULL,
 data LONGTEXT NOT NULL, state VARCHAR(32) NOT NULL,
 received_at BIGINT NOT NULL, updated_at BIGINT NOT NULL,
 claim_token BINARY(16) NULL, claimed_until BIGINT NULL,
 UNIQUE(adapter_id, delivery_key)
);
CREATE INDEX idx_adapter_deliveries_state ON adapter_deliveries(state, received_at);
CREATE TABLE adapter_inspection (
 adapter_id BINARY(16) PRIMARY KEY, mode VARCHAR(32) NOT NULL
);
CREATE TABLE orchestration_debug_controls (
 pipeline_id BINARY(16) PRIMARY KEY, paused BOOLEAN NOT NULL, steps BIGINT NOT NULL
);

ALTER TABLE broker_messages ADD COLUMN adapter_id BINARY(16) NULL;
ALTER TABLE broker_messages ADD COLUMN poll_attempt_id BINARY(16) NULL;
CREATE INDEX idx_broker_messages_adapter ON broker_messages(adapter_id, occurred_at);
