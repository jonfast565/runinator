-- One durable owner for each provider-neutral ingress correlation key.  `org_scope` normalizes
-- the nullable organization id so the unique constraint also protects global definitions.
CREATE TABLE IF NOT EXISTS ingress_admissions (
    id BINARY(16) PRIMARY KEY,
    org_scope VARCHAR(36) NOT NULL,
    org_id BINARY(16) NULL,
    scope VARCHAR(255) NOT NULL,
    correlation_key VARCHAR(512) NOT NULL,
    generation BIGINT NOT NULL,
    target_kind VARCHAR(16) NOT NULL,
    target_id BINARY(16) NOT NULL,
    status VARCHAR(16) NOT NULL,
    workflow_run_id BINARY(16) NULL,
    pipeline_run_id BINARY(16) NULL,
    policy LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    UNIQUE KEY idx_ingress_admissions_key (org_scope, scope, correlation_key),
    KEY idx_ingress_admissions_target (target_kind, target_id)
);
CREATE TABLE IF NOT EXISTS ingress_events (
    id BINARY(16) PRIMARY KEY, admission_id BINARY(16) NOT NULL,
    sequence BIGINT NOT NULL, generation BIGINT NOT NULL,
    source VARCHAR(255) NOT NULL, event_id VARCHAR(512) NOT NULL,
    event_type VARCHAR(255) NOT NULL, correlation_key VARCHAR(512) NOT NULL,
    payload LONGTEXT NOT NULL, occurred_at BIGINT NULL, received_at BIGINT NOT NULL,
    disposition VARCHAR(32) NOT NULL, queue_state VARCHAR(16) NOT NULL,
    claim_token BINARY(16) NULL, promoted_generation BIGINT NULL,
    workflow_run_id BINARY(16) NULL, pipeline_run_id BINARY(16) NULL,
    CONSTRAINT fk_ingress_events_admission FOREIGN KEY (admission_id) REFERENCES ingress_admissions(id) ON DELETE CASCADE,
    UNIQUE KEY idx_ingress_events_dedup (admission_id, source, event_id),
    UNIQUE KEY idx_ingress_events_sequence (admission_id, sequence),
    KEY idx_ingress_events_fifo (admission_id, queue_state, sequence)
);
